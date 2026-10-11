// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The document type declaration.
//!
//! `oxml` does not *validate* — it does not check a document against the
//! content models an `<!ELEMENT>` declares. It does parse the
//! declaration, for two reasons that have nothing to do with validation:
//!
//! 1. **Well-formedness constraints live inside the DTD.** A malformed
//!    `<!ATTLIST>` makes a document not well-formed, and that binds every
//!    parser, validating or not. Skipping to the matching `>` — which is
//!    what this used to do — silently accepts them.
//!
//! 2. **General entities are declared here.** Without reading the
//!    declarations, `&chapter1;` in the body is an undeclared entity and
//!    a perfectly valid document is rejected.
//!
//! Measured against the W3C suite, skipping the DTD accounted for 759
//! wrongly-accepted documents and most of the wrongly-rejected ones.

use alloc::borrow::ToOwned as _;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

/// What a general entity expands to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntityValue {
    /// A literal replacement text from the internal subset.
    Internal(String),
    /// An external *parsed* entity. Its replacement text is not
    /// available — `oxml` never fetches external resources — but the
    /// *declaration* is, which is enough to tell "undeclared" from
    /// "declared but not retrievable".
    ///
    /// Referencing one in an attribute value is forbidden
    /// (`WFC: No External Entity References`); in content it expands to
    /// nothing unless the caller supplied its text.
    ///
    /// The identifiers are kept so that a caller-supplied
    /// [`ExternalSource`] can be asked for the content.
    ///
    /// [`ExternalSource`]: crate::external::ExternalSource
    External {
        /// The `SystemLiteral`.
        system: String,
        /// The `PubidLiteral`, when the declaration used `PUBLIC`.
        public: Option<String>,
    },
    /// An unparsed entity: an `ExternalID` with an `NDataDecl`.
    ///
    /// Referencing one **anywhere** is forbidden
    /// (`WFC: Parsed Entity`). It may only be named as the value of an
    /// `ENTITY`-typed attribute, which is a different construct from a
    /// reference.
    Unparsed,
}

/// Declarations gathered from a document type declaration.
#[derive(Debug, Default, Clone)]
pub(crate) struct Dtd {
    /// General entities, by name, in declaration order of first
    /// definition. A later duplicate declaration is ignored, as the
    /// specification requires.
    pub(crate) general: BTreeMap<String, EntityValue>,
    /// Parameter entities, by name, with their replacement text.
    ///
    /// Only internal ones: an external parameter entity's text is not
    /// available unless the caller supplied it, and a reference to one
    /// leaves the subset incomplete.
    pub(crate) parameters: BTreeMap<String, String>,
    /// External parameter entities the caller supplied, by name.
    ///
    /// `Ok` is the entity's content with its text declaration removed;
    /// `Err` is why that declaration was not acceptable. The error is
    /// carried rather than raised because a processor need not read an
    /// entity nothing references: raising at the declaration rejected
    /// documents whose unreferenced entity happened to be malformed,
    /// which is a rule that only applies to content actually used.
    ///
    /// A name absent here was either not external or not supplied, and
    /// leaves the subset [`Dtd::incomplete`] exactly as before.
    pub(crate) parameters_external:
        BTreeMap<String, Result<String, &'static str>>,
    /// General entities that were declared outside the internal
    /// subset.
    ///
    /// A document saying `standalone="yes"` may not reference one:
    /// XML 1.0 section 2.9 makes that a well-formedness matter, not
    /// merely advice to the application.
    pub(crate) declared_externally: alloc::collections::BTreeSet<String>,
    /// Whether an external subset or a parameter entity was referenced.
    ///
    /// When either is true the internal subset is not the whole story,
    /// so an entity we cannot find may well have been declared
    /// somewhere we did not read. Reporting it as undeclared would
    /// reject valid documents, so entity checking relaxes.
    pub(crate) incomplete: bool,
    /// Whether declarations from outside the internal subset could have
    /// been pulled in.
    ///
    /// Distinct from `incomplete`, which also becomes true merely
    /// because a parameter entity was *declared*. Declaring one pulls
    /// nothing in, so the `WFC: PEs in Internal Subset` rule still
    /// applies afterwards -- conflating the two switched that check off
    /// for every declaration following the first `<!ENTITY % … >`.
    pub(crate) external_seen: bool,
    /// The external subset's identifiers, when the `DOCTYPE` names one.
    ///
    /// Kept so a caller-supplied [`ExternalSource`] can be asked for
    /// its declarations. Without a source it stays unread and
    /// `incomplete` remains true, which is the behaviour that has
    /// always applied.
    ///
    /// [`ExternalSource`]: crate::external::ExternalSource
    pub(crate) external_subset: Option<(String, Option<String>)>,
    /// Attributes declared with type `ID`, as element name to the
    /// attribute names on it that are ID-typed.
    ///
    /// `XPath`'s `id()` selects "the element with the unique ID", and
    /// what makes an attribute an ID is this declaration -- not being
    /// spelled `id`. Without keeping it, `id()` has nothing to match
    /// against and every call answers the empty node-set.
    ///
    /// Keyed by the names as *written*, because that is what the
    /// document body will present too; neither side is namespace
    /// resolved at this point.
    pub(crate) id_attributes: BTreeMap<String, Vec<String>>,
    /// Attributes whose declared type is **not** `CDATA`.
    ///
    /// XML 1.0 section 3.3.3 gives those a second normalisation pass:
    /// leading and trailing spaces are discarded and runs of spaces
    /// collapse to one. A `CDATA` value keeps its spaces exactly.
    ///
    /// The distinction is not cosmetic. `xmlns:b=" urn:x "` declared
    /// `NMTOKEN` normalises to `urn:x`, which binds that prefix to the
    /// same namespace as one declared without the spaces -- and two
    /// attributes whose expanded names then collide make the document
    /// not well-formed. Without this the collision is invisible.
    ///
    /// Keyed by the names as *written*, like `id_attributes`.
    pub(crate) tokenized_attributes: BTreeMap<String, Vec<String>>,
    /// Bytes of parameter-entity replacement text produced so far,
    /// charged against [`Limits::max_pe_expansion`].
    ///
    /// Lives here rather than on the parser because every nested
    /// parser -- one per referenced entity, one per included section
    /// -- shares the one `Dtd`, and a budget each would hand a bomb a
    /// fresh budget per level.
    ///
    /// [`Limits::max_pe_expansion`]: crate::Limits::max_pe_expansion
    pub(crate) pe_spent: usize,
    /// Bytes of declaration text read so far, charged against
    /// [`Limits::max_dtd_bytes`].
    ///
    /// [`Limits::max_dtd_bytes`]: crate::Limits::max_dtd_bytes
    pub(crate) bytes_seen: usize,
}

impl Dtd {
    /// The replacement text of a general entity, if it has one.
    pub(crate) fn entity(&self, name: &str) -> Option<&EntityValue> {
        self.general.get(name)
    }

    /// Whether `attribute` on `element` was declared with a type
    /// other than `CDATA`, and so takes the further normalisation of
    /// XML 1.0 section 3.3.3.
    pub(crate) fn is_tokenized_attribute(
        &self,
        element: &str,
        attribute: &str,
    ) -> bool {
        self.tokenized_attributes
            .get(element)
            .is_some_and(|names| names.iter().any(|n| n == attribute))
    }

    /// Whether `attribute` on `element` was declared with type `ID`.
    pub(crate) fn is_id_attribute(
        &self,
        element: &str,
        attribute: &str,
    ) -> bool {
        self.id_attributes
            .get(element)
            .is_some_and(|names| names.iter().any(|n| n == attribute))
    }
}

/// Attribute defaults declared by `<!ATTLIST>`, used for nothing yet but
/// parsed so their syntax is checked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DefaultDecl {
    Required,
    Implied,
    Fixed,
    Value,
}

/// A cursor over the internal subset.
pub(crate) struct DtdParser<'a> {
    pub(crate) input: &'a str,
    pub(crate) bytes: &'a [u8],
    pub(crate) pos: usize,
    /// The bounds in force. The edition is read from here: name rules
    /// differ between XML 1.0 editions, and names appear in the DTD
    /// too — element and attribute declarations, notation names, and
    /// processing-instruction targets. The rest bound what a
    /// declaration may cost, because the DTD is untrusted input like
    /// everything else.
    pub(crate) limits: crate::Limits,
    /// What the caller made available. This parser opens nothing.
    pub(crate) external: &'a dyn crate::external::ExternalSource,
    /// The document's version, for judging an entity's text
    /// declaration against it.
    pub(crate) version: crate::parser::Version,
}

/// What went wrong, as an offset and a static reason.
pub(crate) type DtdError = (usize, &'static str);

/// Reasons for the bounds this parser applies.
///
/// A `DtdError` carries a static string, and these are the strings a
/// limit produces, so that [`error_kind`] can hand the caller the
/// specific [`ErrorKind`] for the bound rather than a generic
/// `MalformedDtd` -- a limit the caller cannot tell from malformed
/// input is a limit they cannot act on.
///
/// [`ErrorKind`]: crate::ErrorKind
pub(crate) const LIMIT_NAME: &str = "a name exceeds the length limit";
/// See [`LIMIT_NAME`].
pub(crate) const LIMIT_ATTRIBUTE: &str =
    "an attribute default exceeds the size limit";
/// See [`LIMIT_NAME`].
pub(crate) const LIMIT_PE: &str =
    "parameter entity expansion exceeds the limit";
/// See [`LIMIT_NAME`].
pub(crate) const LIMIT_DTD: &str = "the doctype exceeds the size limit";

/// The [`ErrorKind`] a `DtdError` reason stands for.
///
/// Applied by [`Error::new`] to every `MalformedDtd`, so the one place
/// that turns a `DtdError` into an [`Error`] need not know which
/// reasons are limits.
///
/// [`Error`]: crate::Error
/// [`Error::new`]: crate::Error::new
/// [`ErrorKind`]: crate::ErrorKind
pub(crate) fn error_kind(reason: &'static str) -> crate::ErrorKind {
    use crate::ErrorKind;
    match reason {
        LIMIT_NAME => ErrorKind::NameTooLong,
        LIMIT_ATTRIBUTE => ErrorKind::AttributeTooLarge,
        LIMIT_PE => ErrorKind::EntityLimitExceeded,
        LIMIT_DTD => ErrorKind::DtdTooLarge,
        other => ErrorKind::MalformedDtd(other),
    }
}

/// Add `bytes` to a running total and refuse once it passes `max`.
fn charge(
    spent: &mut usize,
    max: usize,
    bytes: usize,
    at: usize,
    reason: &'static str,
) -> Result<(), DtdError> {
    *spent = spent.saturating_add(bytes);
    if *spent > max {
        return Err((at, reason));
    }
    Ok(())
}

impl<'a> DtdParser<'a> {
    /// A parser under the caller's bounds.
    ///
    /// The edition comes from the bounds too: the two travel together
    /// in [`Limits`], and a parser applying one document's edition and
    /// another's budgets would be a mistake.
    ///
    /// [`Limits`]: crate::Limits
    pub(crate) const fn with_limits(
        input: &'a str,
        pos: usize,
        limits: &crate::Limits,
    ) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos,
            limits: *limits,
            // No source and 1.0 unless told otherwise, so a caller that
            // supplies neither behaves exactly as before.
            external: &crate::external::NoExternal,
            version: crate::parser::Version::V10,
        }
    }

    /// A parser over `text`, inheriting this one's bounds, source and
    /// version.
    ///
    /// Every nested parse -- a referenced parameter entity, an
    /// included conditional section, an expanded declaration -- goes
    /// through here so that none of them silently falls back to the
    /// defaults.
    fn nested<'b>(&self, text: &'b str) -> DtdParser<'b>
    where
        'a: 'b,
    {
        DtdParser {
            input: text,
            bytes: text.as_bytes(),
            pos: 0,
            limits: self.limits,
            external: self.external,
            version: self.version,
        }
    }

    /// Charge `bytes` of declaration text against the doctype budget.
    fn charge_dtd_bytes(
        &self,
        dtd: &mut Dtd,
        bytes: usize,
        at: usize,
    ) -> Result<(), DtdError> {
        let max = self.limits.max_dtd_bytes.unwrap_or(usize::MAX);
        charge(&mut dtd.bytes_seen, max, bytes, at, LIMIT_DTD)
    }

    /// Charge `bytes` of replacement text against the parameter-entity
    /// budget.
    fn charge_pe(
        &self,
        dtd: &mut Dtd,
        bytes: usize,
        at: usize,
    ) -> Result<(), DtdError> {
        let max = self.limits.max_pe_expansion;
        charge(&mut dtd.pe_spent, max, bytes, at, LIMIT_PE)
    }

    /// The same parser, able to see what the caller supplied.
    pub(crate) const fn with_external(
        mut self,
        external: &'a dyn crate::external::ExternalSource,
        version: crate::parser::Version,
    ) -> Self {
        self.external = external;
        self.version = version;
        self
    }

    fn is_name_start(&self, c: char) -> bool {
        match self.limits.edition {
            crate::Edition::Fourth => crate::names4e::is_name_start_4e(c),
            _ => crate::parser::is_name_start(c),
        }
    }

    fn is_name_char(&self, c: char) -> bool {
        match self.limits.edition {
            crate::Edition::Fourth => crate::names4e::is_name_char_4e(c),
            _ => crate::parser::is_name_char(c),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn starts_with(&self, s: &str) -> bool {
        self.bytes[self.pos..].starts_with(s.as_bytes())
    }

    fn skip_ws(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            self.pos += 1;
        }
    }

    fn require_ws(&mut self) -> Result<(), DtdError> {
        if !matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
            return Err((self.pos, "whitespace required in a declaration"));
        }
        self.skip_ws();
        Ok(())
    }

    fn expect(&mut self, b: u8, what: &'static str) -> Result<(), DtdError> {
        if self.peek() == Some(b) {
            self.pos += 1;
            Ok(())
        } else {
            Err((self.pos, what))
        }
    }

    /// An XML Name, by the same rules the element parser uses.
    fn name(&mut self) -> Result<&'a str, DtdError> {
        let start = self.pos;
        let rest = &self.input[start..];
        let Some(end) = self.name_end(rest) else {
            return Err((start, "expected a name"));
        };
        // The same bound the element parser applies to the names it
        // reads, so a name the body may not use cannot be declared.
        if end > self.limits.max_name_length {
            return Err((start, LIMIT_NAME));
        }
        self.pos = start + end;
        Ok(&rest[..end])
    }

    /// Where the name beginning `rest` ends, or `None` if no name
    /// begins there.
    fn name_end(&self, rest: &str) -> Option<usize> {
        let mut chars = rest.char_indices();
        let (_, first) = chars.next()?;
        if !self.is_name_start(first) {
            return None;
        }
        Some(
            chars
                .find(|&(_, c)| !self.is_name_char(c))
                .map_or(rest.len(), |(i, _)| i),
        )
    }

    /// A quoted string, returning its contents.
    fn quoted(&mut self) -> Result<&'a str, DtdError> {
        let Some(quote @ (b'"' | b'\'')) = self.peek() else {
            return Err((self.pos, "expected a quoted value"));
        };
        self.pos += 1;
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b == quote {
                let text = &self.input[start..self.pos];
                self.pos += 1;
                return Ok(text);
            }
            self.pos += 1;
        }
        Err((start, "unterminated quoted value"))
    }

    /// Parse an external subset: declarations, with no `<!DOCTYPE`
    /// around them.
    ///
    /// `extSubsetDecl` is the same production the internal subset uses,
    /// so this is the same loop -- it stops at end of input rather than
    /// at `]`.
    pub(crate) fn parse_external_subset(
        &mut self,
        dtd: &mut Dtd,
    ) -> Result<(), DtdError> {
        self.parse_subset_decls(dtd, None, 0)
    }

    /// Parse the whole declaration, starting at `<!DOCTYPE`.
    pub(crate) fn parse_doctype(&mut self) -> Result<Dtd, DtdError> {
        let start = self.pos;
        self.pos += "<!DOCTYPE".len();
        let mut dtd = Dtd::default();

        self.require_ws()?;
        let _root = self.name()?;
        self.skip_ws();

        // ExternalID, if present.
        if self.starts_with("SYSTEM") || self.starts_with("PUBLIC") {
            let is_public = self.starts_with("PUBLIC");
            self.pos += 6;
            self.require_ws()?;
            let ids = if is_public {
                let pubid = self.pubid_literal()?.to_owned();
                // `ExternalID ::= 'PUBLIC' S PubidLiteral S
                // SystemLiteral`. Both the separator and the system
                // literal are required here. `PublicID ::= 'PUBLIC' S
                // PubidLiteral`, without one, is a different
                // production and legal only in a `NotationDecl` --
                // sharing the code let a `DOCTYPE` borrow a rule that
                // is not its own, so `PUBLIC "p""s"` and `PUBLIC "p"`
                // both passed.
                self.require_ws()?;
                Some((self.quoted()?.to_owned(), Some(pubid)))
            } else {
                Some((self.quoted()?.to_owned(), None))
            };
            dtd.external_subset = ids;
            dtd.incomplete = true;
            dtd.external_seen = true;
            self.skip_ws();
        }

        if self.peek() == Some(b'[') {
            self.pos += 1;
            self.parse_internal_subset(&mut dtd)?;
            self.expect(b']', "unterminated internal subset")?;
            self.skip_ws();
        }

        if self.peek() == Some(b'>') {
            self.pos += 1;
            Ok(dtd)
        } else {
            Err((start, "unterminated doctype"))
        }
    }

    /// Whether the declaration starting here references a parameter
    /// entity outside a quoted literal.
    ///
    /// A `%` inside a literal is data -- `<!ENTITY x "50%">` declares an
    /// entity and references nothing.
    fn decl_uses_pe(&self) -> bool {
        let mut depth = 0usize;
        let mut quote: Option<u8> = None;
        for (i, &b) in self.bytes[self.pos..].iter().enumerate() {
            let rest = &self.bytes[self.pos + i + 1..];
            match (quote, b) {
                (Some(q), c) if c == q => quote = None,
                (None, b'"' | b'\'') => quote = Some(b),
                (None, b'<') => depth += 1,
                (None, b'>') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return false;
                    }
                }
                // `%Name;` is a *reference*. `<!ENTITY % name …>` is a
                // parameter entity *declaration*, where `%` is syntax
                // and the grammar requires whitespace after it. Not
                // telling them apart meant every parameter entity
                // declaration was skipped as unresolvable -- so nothing
                // was ever declared to expand.
                (None, b'%') => return next_is_name_start(rest),
                _ => {}
            }
        }
        false
    }

    /// Skip past a declaration without interpreting it.
    ///
    /// Running out of input is expected rather than malformed: the
    /// declaration's own `>` may be supplied by the parameter entity we
    /// declined to expand -- `<!ENTITY % e ">">` followed by
    /// `<!ELEMENT doc (#PCDATA) %e;` is a real pattern in the suite.
    /// The subset is already known to be only partly understood.
    fn skip_decl(&mut self) {
        let mut depth = 0usize;
        let mut quote: Option<u8> = None;
        while let Some(b) = self.peek() {
            self.pos += 1;
            match (quote, b) {
                (Some(q), c) if c == q => quote = None,
                (None, b'"' | b'\'') => quote = Some(b),
                (None, b'<') => depth += 1,
                (None, b'>') => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return;
                    }
                }
                _ => {}
            }
        }
    }

    /// A conditional section, after its `<![`.
    ///
    /// ```text
    /// includeSect ::= '<![' S? 'INCLUDE' S? '[' extSubsetDecl ']]>'
    /// ignoreSect  ::= '<![' S? 'IGNORE'  S? '[' ignoreSectContents* ']]>'
    /// ```
    ///
    /// Those are the only two keywords. Skipping the whole section
    /// without reading it accepted any word at all -- and skipped the
    /// declarations an `INCLUDE` section contains, so they were never
    /// seen either.
    fn parse_conditional_section(
        &mut self,
        dtd: &mut Dtd,
        depth: usize,
    ) -> Result<(), DtdError> {
        let Some((keyword, bracket_from_entity)) =
            self.conditional_keyword(dtd)?
        else {
            return Ok(());
        };
        if !bracket_from_entity {
            self.skip_ws();
            self.expect(b'[', "expected `[` in a conditional section")?;
        }
        let body_start = self.pos;
        self.skip_conditional_section()?;
        // `skip_conditional_section` consumed the closing `]]>`.
        let body = &self.input[body_start..self.pos - 3];

        match keyword.as_str() {
            "IGNORE" => Ok(()),
            "INCLUDE" => {
                self.nested(body).parse_subset_decls(dtd, None, depth + 1)
            }
            _ => Err((
                body_start,
                "a conditional section must say INCLUDE or IGNORE",
            )),
        }
    }

    /// The keyword of a conditional section, and whether the `[` after
    /// it came along with it.
    ///
    /// The keyword may arrive through a parameter entity:
    /// `<![%MAYBE;[ … ]]>` is how the suite tests both branches from
    /// one document. A parameter entity is replacement *text*, not a
    /// token, so it may carry the opening bracket with it:
    /// `<!ENTITY % e "INCLUDE[">` used as `<![ %e; … ]]>` is in the
    /// suite and is legal. Requiring a `[` after the reference
    /// rejected it.
    ///
    /// `None` when the entity is unknown: the section's fate is
    /// undecidable, so it has been skipped as ignored and recorded as
    /// unread.
    fn conditional_keyword(
        &mut self,
        dtd: &mut Dtd,
    ) -> Result<Option<(String, bool)>, DtdError> {
        self.skip_ws();
        if self.peek() != Some(b'%') {
            return Ok(Some((self.name()?.to_owned(), false)));
        }
        self.pos += 1;
        let name = self.name()?.to_owned();
        self.expect(b';', "unterminated parameter entity reference")?;
        let Some(text) = self.parameter_text(dtd, &name, self.pos)? else {
            dtd.incomplete = true;
            self.skip_conditional_section()?;
            return Ok(None);
        };
        let text = text.trim();
        Ok(Some(match text.strip_suffix('[') {
            Some(keyword) => (keyword.trim().to_owned(), true),
            None => (text.to_owned(), false),
        }))
    }

    /// Skip a conditional section, counting nested `<![` and `]]>`.
    ///
    /// `ignoreSectContents` says the delimiters must be **balanced**
    /// inside an ignored section, so stopping at the first `]]>` is
    /// wrong whenever the ignored text contains one -- which the suite
    /// tests with a section whose prose quotes the delimiters it is
    /// explaining. Everything after that point was then misaligned.
    fn skip_conditional_section(&mut self) -> Result<(), DtdError> {
        let start = self.pos;
        let mut depth = 1usize;
        while self.pos < self.bytes.len() {
            if self.starts_with("<![") {
                depth += 1;
                self.pos += 3;
            } else if self.starts_with("]]>") {
                depth -= 1;
                self.pos += 3;
                if depth == 0 {
                    return Ok(());
                }
            } else {
                self.pos += 1;
            }
        }
        Err((start, "unterminated conditional section"))
    }

    fn parse_internal_subset(&mut self, dtd: &mut Dtd) -> Result<(), DtdError> {
        self.parse_subset_decls(dtd, Some(b']'), 0)
    }

    /// The shared `extSubsetDecl` loop.
    ///
    /// `terminator` is `Some(b']')` for an internal subset, which must
    /// be closed, and `None` for an external one, which simply ends.
    fn parse_subset_decls(
        &mut self,
        dtd: &mut Dtd,
        terminator: Option<u8>,
        depth: usize,
    ) -> Result<(), DtdError> {
        loop {
            self.skip_ws();
            let start = self.pos;
            match self.peek() {
                None if terminator.is_none() => return Ok(()),
                None => return Err((self.pos, "unterminated internal subset")),
                Some(b) if Some(b) == terminator => return Ok(()),
                Some(b'%') => self.subset_pe_reference(dtd, depth)?,
                Some(b'<') => self.subset_markup(dtd, terminator, depth)?,
                _ => return Err((self.pos, "expected a markup declaration")),
            }
            // Only the outermost loop reads declaration text from the
            // document or an external subset; a nested one reads
            // replacement text, which the parameter-entity budget has
            // already paid for.
            if depth == 0 {
                self.charge_dtd_bytes(dtd, self.pos - start, start)?;
            }
        }
    }

    /// A parameter-entity reference between declarations.
    ///
    /// A parameter-entity reference can expand to anything, including
    /// further declarations, so once one appears the subset is no
    /// longer fully known to us.
    fn subset_pe_reference(
        &mut self,
        dtd: &mut Dtd,
        depth: usize,
    ) -> Result<(), DtdError> {
        self.pos += 1;
        let name = self.name()?.to_owned();
        self.expect(b';', "unterminated parameter entity reference")?;
        dtd.external_seen = true;
        // The replacement text of a known parameter entity is
        // `extSubsetDecl` in its own right, so it is parsed as one.
        // Only an entity we do not have leaves the subset incomplete.
        match self.parameter_text(dtd, &name, self.pos)? {
            Some(text) if depth < MAX_PE_DEPTH => {
                self.nested(&text).parse_subset_decls(dtd, None, depth + 1)
            }
            Some(_) => Err((self.pos, "parameter entities nested too deeply")),
            None => {
                dtd.incomplete = true;
                Ok(())
            }
        }
    }

    /// Something beginning `<` between declarations: a conditional
    /// section, a declaration that references a parameter entity, or
    /// a plain markup declaration.
    ///
    /// A conditional section is not a markup declaration, and its
    /// keyword arriving through a parameter entity is the normal way
    /// to write one: `<![%MAYBE;[ … ]]>`. Letting it fall into the
    /// branch below sent it down a path that reports an unparseable
    /// declaration as merely *incomplete*, so a section saying `CDATA`
    /// where only INCLUDE or IGNORE is legal was accepted in silence.
    /// `parse_conditional_section` resolves the keyword itself and
    /// rejects the ones that are not keywords.
    fn subset_markup(
        &mut self,
        dtd: &mut Dtd,
        terminator: Option<u8>,
        depth: usize,
    ) -> Result<(), DtdError> {
        if terminator.is_none() && self.starts_with("<![") {
            self.pos += 3;
            return self.parse_conditional_section(dtd, depth);
        }
        if terminator.is_some() || !self.decl_uses_pe() {
            return self.parse_markup_decl(dtd, depth);
        }
        self.expanded_markup_decl(dtd, depth)
    }

    /// A declaration whose text references a parameter entity.
    ///
    /// It cannot be parsed without expanding it. In the external
    /// subset that is legal and common -- `<!ELEMENT x (a,%choice;,c)>`
    /// -- so what we know is expanded and the result parsed. A
    /// reference we cannot resolve leaves the declaration unparsed
    /// rather than reported as malformed: better an unchecked
    /// constraint than a rejected document.
    fn expanded_markup_decl(
        &mut self,
        dtd: &mut Dtd,
        depth: usize,
    ) -> Result<(), DtdError> {
        let start = self.pos;
        self.skip_decl();
        let raw = &self.input[start..self.pos];
        let mut budget = PeBudget {
            spent: dtd.pe_spent,
            max: self.limits.max_pe_expansion,
            at: start,
        };
        let expanded = expand_pes(raw, dtd, depth, &mut budget);
        dtd.pe_spent = budget.spent;
        let parsed = match expanded? {
            Some(text) => {
                self.nested(&text).parse_markup_decl(dtd, depth).is_ok()
            }
            None => false,
        };
        if !parsed {
            dtd.incomplete = true;
        }
        Ok(())
    }

    fn parse_markup_decl(
        &mut self,
        dtd: &mut Dtd,
        depth: usize,
    ) -> Result<(), DtdError> {
        if self.starts_with("<!--") {
            self.pos += 4;
            return match self.input[self.pos..].find("-->") {
                Some(i) => {
                    let body = &self.input[self.pos..self.pos + i];
                    if body.contains("--") || body.ends_with('-') {
                        return Err((self.pos, "`--` inside a comment"));
                    }
                    self.pos += i + 3;
                    Ok(())
                }
                None => Err((self.pos, "unterminated comment")),
            };
        }
        if self.starts_with("<?") {
            self.pos += 2;
            // The target is a Name, and its legality depends on the
            // edition. Skipping to `?>` accepted illegal targets — a
            // whole class of not-well-formed documents that the suite
            // tests for at production 85.
            let target = self.name()?;
            if target.eq_ignore_ascii_case("xml") {
                return Err((self.pos, "`xml` is a reserved PI target"));
            }
            // `PI ::= '<?' PITarget (S (Char* - (Char* '?>' Char*)))? '?>'`
            // — the target must be followed by whitespace or the close.
            // Without this the name simply stops at the first illegal
            // character and the rest is swallowed as data, accepting a
            // document that is not well-formed.
            if !matches!(self.peek(), Some(b' ' | b'\t' | b'\r' | b'\n'))
                && !self.starts_with("?>")
            {
                return Err((self.pos, "illegal character in a PI target"));
            }
            return match self.input[self.pos..].find("?>") {
                Some(i) => {
                    self.pos += i + 2;
                    Ok(())
                }
                None => Err((self.pos, "unterminated processing instruction")),
            };
        }
        // Conditional sections are only legal in the external subset,
        // but a parameter entity can bring one in, so accept them
        // whenever the subset is already known to be incomplete.
        if self.starts_with("<![") {
            if !dtd.incomplete {
                return Err((
                    self.pos,
                    "conditional section in the internal subset",
                ));
            }
            self.pos += 3;
            return self.parse_conditional_section(dtd, depth);
        }
        if self.starts_with("<!ELEMENT") {
            self.pos += "<!ELEMENT".len();
            return self.parse_element_decl();
        }
        if self.starts_with("<!ATTLIST") {
            self.pos += "<!ATTLIST".len();
            return self.parse_attlist_decl(dtd);
        }
        if self.starts_with("<!ENTITY") {
            self.pos += "<!ENTITY".len();
            return self.parse_entity_decl(dtd);
        }
        if self.starts_with("<!NOTATION") {
            self.pos += "<!NOTATION".len();
            return self.parse_notation_decl();
        }
        Err((self.pos, "unknown markup declaration"))
    }

    fn parse_element_decl(&mut self) -> Result<(), DtdError> {
        self.require_ws()?;
        let _ = self.name()?;
        self.require_ws()?;
        if self.peek() == Some(b'(') {
            self.parse_content_spec()?;
        } else {
            let kw = self.name()?;
            if !matches!(kw, "EMPTY" | "ANY") {
                return Err((
                    self.pos,
                    "expected EMPTY, ANY or a content model",
                ));
            }
        }
        self.skip_ws();
        self.expect(b'>', "unterminated element declaration")
    }

    /// `contentspec` — the parenthesised part of an `<!ELEMENT>`.
    ///
    /// ```text
    /// Mixed    ::= '(' S? '#PCDATA' (S? '|' S? Name)* S? ')*'
    ///            | '(' S? '#PCDATA' S? ')'
    /// children ::= (choice | seq) ('?' | '*' | '+')?
    /// cp       ::= (Name | choice | seq) ('?' | '*' | '+')?
    /// choice   ::= '(' S? cp ( S? '|' S? cp )+ S? ')'
    /// seq      ::= '(' S? cp ( S? ',' S? cp )* S? ')'
    /// ```
    ///
    /// Matching brackets is not enough. `(doc|#PCDATA)*` balances but is
    /// not a legal model — `#PCDATA` must come first — and a group may
    /// not mix `|` with `,`. Skipping to the closing parenthesis
    /// accepted both.
    fn parse_content_spec(&mut self) -> Result<(), DtdError> {
        self.pos += 1; // '('
        self.skip_ws();
        if self.starts_with("#PCDATA") {
            self.pos += "#PCDATA".len();
            return self.parse_mixed_tail();
        }
        self.parse_group_tail()?;
        if matches!(self.peek(), Some(b'?' | b'*' | b'+')) {
            self.pos += 1;
        }
        Ok(())
    }

    /// The rest of a `Mixed` model, after `#PCDATA`.
    fn parse_mixed_tail(&mut self) -> Result<(), DtdError> {
        let mut had_names = false;
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'|') => {
                    self.pos += 1;
                    self.skip_ws();
                    let _ = self.name()?;
                    had_names = true;
                }
                Some(b')') => {
                    self.pos += 1;
                    // With alternatives the model must be starred;
                    // `(#PCDATA)` alone must not be.
                    if had_names {
                        if self.peek() == Some(b'*') {
                            self.pos += 1;
                            return Ok(());
                        }
                        return Err((
                            self.pos,
                            "a mixed model with names must end in `)*`",
                        ));
                    }
                    if self.peek() == Some(b'*') {
                        self.pos += 1;
                    }
                    return Ok(());
                }
                _ => return Err((self.pos, "malformed mixed content model")),
            }
        }
    }

    /// A `choice` or `seq`, after its opening parenthesis.
    fn parse_group_tail(&mut self) -> Result<(), DtdError> {
        // `None` until the first separator fixes which kind this is.
        let mut separator: Option<u8> = None;
        loop {
            self.skip_ws();
            // Each particle is a name or a nested group.
            if self.peek() == Some(b'(') {
                self.pos += 1;
                self.parse_group_tail()?;
            } else {
                let _ = self.name()?;
            }
            if matches!(self.peek(), Some(b'?' | b'*' | b'+')) {
                self.pos += 1;
            }
            self.skip_ws();
            match self.peek() {
                Some(sep @ (b'|' | b',')) => {
                    // A group is a choice or a sequence, never both.
                    if separator.is_some_and(|s| s != sep) {
                        return Err((
                            self.pos,
                            "a content model group cannot mix `|` and `,`",
                        ));
                    }
                    separator = Some(sep);
                    self.pos += 1;
                }
                Some(b')') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err((self.pos, "malformed content model")),
            }
        }
    }

    fn parse_attlist_decl(&mut self, dtd: &mut Dtd) -> Result<(), DtdError> {
        self.require_ws()?;
        let element = self.name()?;
        loop {
            self.skip_ws();
            if self.peek() == Some(b'>') {
                self.pos += 1;
                return Ok(());
            }
            if self.peek().is_none() {
                return Err((self.pos, "unterminated attribute list"));
            }
            // AttDef: Name AttType DefaultDecl
            let attribute = self.name()?;
            self.require_ws()?;
            let (is_id, is_cdata) = self.parse_att_type()?;
            self.require_ws()?;
            let _ = self.parse_default_decl(dtd)?;
            if !is_cdata {
                let names = dtd
                    .tokenized_attributes
                    .entry(element.to_owned())
                    .or_default();
                if !names.iter().any(|n| n == attribute) {
                    names.push(attribute.to_owned());
                }
            }
            if is_id {
                let names =
                    dtd.id_attributes.entry(element.to_owned()).or_default();
                // A repeated declaration of the same attribute is
                // ignored rather than stored twice, matching how a
                // duplicate entity declaration is treated.
                if !names.iter().any(|n| n == attribute) {
                    names.push(attribute.to_owned());
                }
            }
        }
    }

    /// Parse an `AttType`, reporting `(is_id, is_cdata)`.
    ///
    /// Both matter and they are not opposites. `ID` decides what
    /// `XPath`'s `id()` can find; `CDATA` decides whether the value
    /// keeps its spaces. An enumeration and `NOTATION` are neither
    /// `ID` nor `CDATA`, so they are tokenized too -- which is easy to
    /// miss, because they are the two forms written with parentheses
    /// rather than a keyword.
    fn parse_att_type(&mut self) -> Result<(bool, bool), DtdError> {
        if self.peek() == Some(b'(') {
            return self.parse_enumeration(false).map(|()| (false, false));
        }
        let kw = self.name()?;
        match kw {
            "ID" => Ok((true, false)),
            "CDATA" => Ok((false, true)),
            "IDREF" | "IDREFS" | "ENTITY" | "ENTITIES" | "NMTOKEN"
            | "NMTOKENS" => Ok((false, false)),
            "NOTATION" => {
                self.require_ws()?;
                if self.peek() != Some(b'(') {
                    return Err((self.pos, "NOTATION needs a name list"));
                }
                self.parse_enumeration(true).map(|()| (false, false))
            }
            _ => Err((self.pos, "unknown attribute type")),
        }
    }

    /// `Enumeration ::= '(' S? Nmtoken (S? '|' S? Nmtoken)* S? ')'`
    /// and `NotationType ::= 'NOTATION' S '(' S? Name (S? '|' S? Name)* S? ')'`
    ///
    /// Both are non-empty, so `()` is not an empty enumeration but a
    /// syntax error — which counting brackets could not tell. A
    /// notation list holds Names; a plain enumeration holds Nmtokens,
    /// which unlike Names may begin with a digit.
    fn parse_enumeration(&mut self, names_only: bool) -> Result<(), DtdError> {
        self.pos += 1; // '('
        loop {
            self.skip_ws();
            if names_only {
                let _ = self.name()?;
            } else {
                let start = self.pos;
                while self.input[self.pos..]
                    .chars()
                    .next()
                    .is_some_and(|c| self.is_name_char(c))
                {
                    self.pos += self.input[self.pos..]
                        .chars()
                        .next()
                        .map_or(0, char::len_utf8);
                }
                if self.pos == start {
                    return Err((self.pos, "empty item in an enumeration"));
                }
            }
            self.skip_ws();
            match self.peek() {
                Some(b'|') => self.pos += 1,
                Some(b')') => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => return Err((self.pos, "malformed enumeration")),
            }
        }
    }

    /// A `PubidLiteral`, whose characters are restricted to
    /// `PubidChar ::= #x20 | #xD | #xA | [a-zA-Z0-9] | [-'()+,./:=?;!*#@$_%]`.
    fn pubid_literal(&mut self) -> Result<&'a str, DtdError> {
        let start = self.pos;
        let text = self.quoted()?;
        if let Some(bad) = text.chars().find(|c| !is_pubid_char(*c)) {
            let _ = bad;
            return Err((start, "illegal character in a public identifier"));
        }
        Ok(text)
    }

    fn parse_default_decl(
        &mut self,
        dtd: &Dtd,
    ) -> Result<DefaultDecl, DtdError> {
        if self.peek() == Some(b'#') {
            self.pos += 1;
            let kw = self.name()?;
            return match kw {
                "REQUIRED" => Ok(DefaultDecl::Required),
                "IMPLIED" => Ok(DefaultDecl::Implied),
                "FIXED" => {
                    self.require_ws()?;
                    let start = self.pos;
                    let text = self.quoted()?;
                    self.validate_attribute_default(text, start, dtd)?;
                    Ok(DefaultDecl::Fixed)
                }
                _ => Err((self.pos, "expected #REQUIRED, #IMPLIED or #FIXED")),
            };
        }
        let start = self.pos;
        let text = self.quoted()?;
        self.validate_attribute_default(text, start, dtd)?;
        Ok(DefaultDecl::Value)
    }

    /// Check an `<!ATTLIST>` default value.
    ///
    /// A default is an `AttValue`, and the well-formedness constraints
    /// that apply to an attribute value in a document apply to it
    /// here -- at the point of *declaration*, whether or not any
    /// element ever uses the default. None of them were checked,
    /// because the value was parsed with `quoted()` and thrown away.
    /// So does the size bound: a default is an attribute value the
    /// body never has to spell out.
    fn validate_attribute_default(
        &self,
        text: &str,
        offset: usize,
        dtd: &Dtd,
    ) -> Result<(), DtdError> {
        if text.len() > self.limits.max_attribute_size {
            return Err((offset, LIMIT_ATTRIBUTE));
        }
        let mut visiting = Vec::new();
        check_att_value(text, offset, dtd, &mut visiting)
    }

    fn parse_entity_decl(&mut self, dtd: &mut Dtd) -> Result<(), DtdError> {
        self.require_ws()?;
        // A parameter entity declaration: `<!ENTITY % name ...>`.
        let parameter = if self.peek() == Some(b'%') {
            self.pos += 1;
            self.require_ws()?;
            true
        } else {
            false
        };
        let name_at = self.pos;
        let name = self.name()?;
        // Namespaces in XML narrows these from `Name` to `NCName`, so
        // `<!ENTITY a:b …>` is not legal. XML's own grammar allows the
        // colon, which is why it needs saying.
        if name.contains(':') {
            return Err((name_at, "a colon is not allowed in an entity name"));
        }
        self.require_ws()?;

        let value = if matches!(self.peek(), Some(b'"' | b'\'')) {
            let start = self.pos;
            let text = self.quoted()?;
            // An `EntityValue` is checked when it is *declared*, not
            // only when it is used: a malformed character reference
            // makes the document not well-formed even if nothing ever
            // references the entity.
            validate_entity_value(text, start, dtd.external_seen)?;
            EntityValue::Internal(text.to_owned())
        } else {
            let is_public = self.starts_with("PUBLIC");
            if !is_public && !self.starts_with("SYSTEM") {
                return Err((
                    self.pos,
                    "expected an entity value or ExternalID",
                ));
            }
            self.pos += 6;
            self.require_ws()?;
            let (system, public) = if is_public {
                let pubid = self.pubid_literal()?.to_owned();
                self.require_ws()?;
                (self.quoted()?.to_owned(), Some(pubid))
            } else {
                (self.quoted()?.to_owned(), None)
            };
            // NDataDecl, for an unparsed entity. Kept apart from an
            // external *parsed* entity because the two are forbidden in
            // different places: an unparsed entity may not be
            // referenced anywhere, an external parsed one only in an
            // attribute value.
            //
            // `NDataDecl ::= S 'NDATA' S Name` — the leading whitespace
            // is part of the production, so `"image.jpg"NDATA n` is not
            // an entity with a notation, it is malformed. Skipping
            // whitespace before looking accepted both.
            let before_ws = self.pos;
            self.skip_ws();
            let had_ws = self.pos > before_ws;
            if self.starts_with("NDATA") {
                if !had_ws {
                    return Err((
                        self.pos,
                        "whitespace is required before NDATA",
                    ));
                }
                // `PEDef ::= EntityValue | ExternalID` — a parameter
                // entity has no NDataDecl. Only a general entity can be
                // unparsed.
                if parameter {
                    return Err((
                        self.pos,
                        "a parameter entity may not have an NDataDecl",
                    ));
                }
                self.pos += "NDATA".len();
                self.require_ws()?;
                let _ = self.name()?;
                EntityValue::Unparsed
            } else {
                EntityValue::External { system, public }
            }
        };

        self.skip_ws();
        self.expect(b'>', "unterminated entity declaration")?;

        if parameter {
            // The replacement text of a parameter entity can contain
            // declarations we have not expanded, so treat its presence
            // as making the entity set incomplete. It does **not** make
            // the subset external: nothing has been pulled in until the
            // entity is referenced, so `WFC: PEs in Internal Subset`
            // still applies to every declaration after this one.
            dtd.incomplete = true;
            if let EntityValue::External { system, public } = &value {
                self.fetch_external_pe(dtd, name, system, public.as_deref())?;
            }
            if let EntityValue::Internal(text) = &value {
                // Character references in an entity value are expanded
                // when it is **declared**, not when it is used. The
                // specification's own "tricky" example turns on this:
                // `<!ENTITY % xx '&#37;zz;'>` stores `%zz;`, so
                // referencing `xx` references `zz`. Storing the raw
                // text left the chain inert.
                let expanded = expand_char_refs(text);
                let _ =
                    dtd.parameters.entry(name.to_owned()).or_insert(expanded);
            }
        } else {
            // "the first declaration binds" — a later duplicate is not
            // an error, it is ignored.
            let _ = dtd.general.entry(name.to_owned()).or_insert(value);
        }
        Ok(())
    }

    /// Fetch an external parameter entity's content, if the caller can
    /// supply it.
    ///
    /// Fetched now, judged later. Only the caller can supply it -- this
    /// parser opens nothing -- and a name it does not know simply
    /// stays unresolved. What arrives counts against the doctype
    /// budget before it is held: nothing else bounds what a source
    /// hands back.
    fn fetch_external_pe(
        &self,
        dtd: &mut Dtd,
        name: &str,
        system: &str,
        public: Option<&str>,
    ) -> Result<(), DtdError> {
        let Some(content) = self.external.fetch(system, public) else {
            return Ok(());
        };
        self.charge_dtd_bytes(dtd, content.len(), self.pos)?;
        let resolved = self.text_declaration(content);
        let _ = dtd
            .parameters_external
            .entry(name.to_owned())
            .or_insert(resolved);
        Ok(())
    }

    /// The replacement text of a parameter entity, wherever it lives.
    ///
    /// Internal entities are held as text; external ones were fetched
    /// when they were declared and are judged **here**, at the
    /// reference, because an entity nothing references need not be
    /// read at all. `None` means the name is unknown or was never
    /// supplied, which leaves the subset incomplete rather than wrong.
    ///
    /// Each reference is charged the length of the text it produces,
    /// as a general entity reference is: a parameter entity referring
    /// to another a hundred times, three levels deep, is ten thousand
    /// copies of the innermost from a few hundred bytes, and depth
    /// alone never notices.
    fn parameter_text(
        &self,
        dtd: &mut Dtd,
        name: &str,
        at: usize,
    ) -> Result<Option<String>, DtdError> {
        let text = match dtd.parameters.get(name) {
            Some(text) => text.clone(),
            None => match dtd.parameters_external.get(name) {
                Some(Ok(body)) => body.clone(),
                Some(Err(reason)) => return Err((at, reason)),
                None => return Ok(None),
            },
        };
        self.charge_pe(dtd, text.len(), at)?;
        Ok(Some(text))
    }

    /// An external parameter entity's content, minus its text
    /// declaration, or why the declaration was not acceptable.
    ///
    /// `TextDecl ::= '<?xml' VersionInfo? EncodingDecl S? '?>'` — the
    /// version is optional but must precede the encoding, the encoding
    /// is required, `standalone` belongs to a document and not to an
    /// entity, and the whole thing must come first. The suite tests
    /// each of those separately, which is why this defers to the same
    /// checks the external *subset* path uses rather than restating
    /// them.
    fn text_declaration(&self, content: &str) -> Result<String, &'static str> {
        // Each entity is normalised by its own declared version: one
        // declaring 1.1 may use U+2028 as whitespace, including inside
        // its own text declaration.
        let version = crate::parser::entity_version(content, self.version);
        let normalized =
            crate::parser::normalize_line_endings(content, version);
        crate::parser::check_text_decl_position(&normalized, 0)
            .map_err(|_| "a text declaration must come first")?;
        crate::parser::check_text_decl(&normalized, self.version, 0)
            .map_err(|_| "malformed text declaration")?;
        let body = crate::parser::strip_text_decl(&normalized);
        // Judged by both versions, as the external *subset* path is.
        // The entity's own version rules its characters, and so does
        // the version of the document pulling it in: `#x8F` is invalid
        // in XML 1.1 whatever the entity declares, and nothing checked
        // an external parameter entity's characters at all.
        if body.chars().any(|c| {
            !crate::parser::is_literal_char_for(c, version)
                || !crate::parser::is_literal_char_for(c, self.version)
        }) {
            return Err("a character the document's version forbids");
        }
        Ok(body.to_owned())
    }

    fn parse_notation_decl(&mut self) -> Result<(), DtdError> {
        self.require_ws()?;
        let name_at = self.pos;
        let name = self.name()?;
        if name.contains(':') {
            return Err((name_at, "a colon is not allowed in a notation name"));
        }
        self.require_ws()?;
        let public = self.starts_with("PUBLIC");
        if !public && !self.starts_with("SYSTEM") {
            return Err((self.pos, "notation needs SYSTEM or PUBLIC"));
        }
        self.pos += 6;
        self.require_ws()?;
        if public {
            let _ = self.pubid_literal()?;
        } else {
            let _ = self.quoted()?;
        }
        self.skip_ws();
        if public && matches!(self.peek(), Some(b'"' | b'\'')) {
            let _ = self.quoted()?;
            self.skip_ws();
        }
        self.expect(b'>', "unterminated notation declaration")
    }
}

/// Expand character references in an entity's replacement text.
///
/// Done when the entity is declared, per XML section 4.5: `&#37;`
/// becomes `%` and `&#60;` becomes `<`, so a parameter entity can be
/// built that expands to a declaration. General entity references are
/// **not** expanded here -- they are bypassed and left for use.
fn expand_char_refs(text: &str) -> String {
    if !text.contains("&#") {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find("&#") {
        let (before, tail) = rest.split_at(i);
        out.push_str(before);
        let after = &tail[2..];
        let Some(semi) = after.find(';') else {
            out.push_str(tail);
            return out;
        };
        let digits = &after[..semi];
        let value = if let Some(hex) = digits.strip_prefix(['x', 'X']) {
            u32::from_str_radix(hex, 16).ok()
        } else {
            digits.parse::<u32>().ok()
        };
        match value.and_then(char::from_u32) {
            Some(c) => out.push(c),
            // Malformed references are reported by
            // `validate_entity_value`; leave the text alone here.
            None => {
                out.push_str(&tail[..semi + 3]);
            }
        }
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    out
}

/// Whether a parameter entity reference's name starts here.
///
/// Only the ASCII range is checked, which is enough to tell a reference
/// from the whitespace that must follow the `%` of a declaration.
fn next_is_name_start(rest: &[u8]) -> bool {
    matches!(rest.first(), Some(b) if b.is_ascii_alphabetic()
        || *b == b'_' || *b == b':' || *b >= 0x80)
}

/// How deep parameter entity expansion may nest.
///
/// A parameter entity whose text references itself would otherwise
/// recurse forever, and the DTD is untrusted input like everything
/// else.
const MAX_PE_DEPTH: usize = 40;

/// The parameter-entity budget while one declaration is expanded.
///
/// Carried apart from the [`Dtd`] it belongs to because expansion
/// reads the entity table while it charges, and the two cannot be
/// borrowed from one struct at once.
struct PeBudget {
    /// Bytes produced so far, across the whole document.
    spent: usize,
    /// [`Limits::max_pe_expansion`].
    ///
    /// [`Limits::max_pe_expansion`]: crate::Limits::max_pe_expansion
    max: usize,
    /// Where the declaration being expanded begins, for the error.
    at: usize,
}

/// Replace every `%name;` in `text` with its replacement text.
///
/// `Ok(None)` when a reference names an entity we do not have, or when
/// expansion nests too deeply -- the caller then leaves the declaration
/// unparsed rather than guessing at it. `Err` only when the budget is
/// spent: that is not an unknown to shrug at but an attack to refuse.
fn expand_pes(
    text: &str,
    dtd: &Dtd,
    depth: usize,
    budget: &mut PeBudget,
) -> Result<Option<String>, DtdError> {
    if depth >= MAX_PE_DEPTH {
        return Ok(None);
    }
    if !text.contains('%') {
        return Ok(Some(text.to_owned()));
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let mut quote: Option<char> = None;
    while let Some(i) = rest.find(['%', '"', '\'']) {
        let (before, tail) = rest.split_at(i);
        out.push_str(before);
        let c = tail.chars().next().unwrap_or('%');
        if quote.is_some() || c != '%' {
            // A quote opens or closes a literal, and a `%` inside one
            // is data, not a reference.
            quote = toggle_quote(quote, c);
            out.push(c);
            rest = &tail[c.len_utf8()..];
            continue;
        }
        let Some((value, after)) = pe_reference(&tail[1..], dtd) else {
            return Ok(None);
        };
        charge(
            &mut budget.spent,
            budget.max,
            value.len(),
            budget.at,
            LIMIT_PE,
        )?;
        let Some(expanded) = expand_pes(value, dtd, depth + 1, budget)? else {
            return Ok(None);
        };
        out.push_str(&expanded);
        rest = after;
    }
    out.push_str(rest);
    Ok(Some(out))
}

/// The quote state after seeing `c`: a matching quote closes the
/// literal, any quote opens one, anything else changes nothing.
fn toggle_quote(quote: Option<char>, c: char) -> Option<char> {
    match quote {
        Some(q) if q == c => None,
        Some(q) => Some(q),
        None if matches!(c, '"' | '\'') => Some(c),
        None => None,
    }
}

/// Resolve the `name;` of a reference: its replacement text and what
/// follows the `;`, or `None` for a reference we cannot resolve.
fn pe_reference<'d>(
    after: &'d str,
    dtd: &'d Dtd,
) -> Option<(&'d str, &'d str)> {
    let semi = after.find(';')?;
    let value = dtd.parameters.get(&after[..semi])?;
    Some((value.as_str(), &after[semi + 1..]))
}

/// `PubidChar`, XML 1.0 production 13.
fn is_pubid_char(c: char) -> bool {
    matches!(c, '\u{20}' | '\u{D}' | '\u{A}' | 'a'..='z' | 'A'..='Z' | '0'..='9')
        || "-'()+,./:=?;!*#@$_%".contains(c)
}

fn check_att_value<'n>(
    text: &'n str,
    offset: usize,
    dtd: &'n Dtd,
    visiting: &mut Vec<&'n str>,
) -> Result<(), DtdError> {
    // `AttValue ::= '"' ([^<&"] | Reference)* '"'`. A literal `<` is
    // forbidden here exactly as it is in a document.
    if text.contains('<') {
        return Err((offset, "`<` is not allowed in an attribute value"));
    }

    let mut rest = text;
    while let Some(i) = rest.find('&') {
        let tail = &rest[i + 1..];
        let Some(semi) = tail.find(';') else {
            return Err((offset, "unterminated reference in a default value"));
        };
        let name = &tail[..semi];
        rest = &tail[semi + 1..];

        // Character references stand for characters and are always
        // fine; `validate_entity_value`'s rules cover their syntax.
        if name.starts_with('#') || decode_predefined_name(name) {
            continue;
        }

        // `WFC: No Recursion`.
        if visiting.contains(&name) {
            return Err((
                offset,
                "an entity in a default value refers to itself",
            ));
        }

        match dtd.general.get(name) {
            // `WFC: Entity Declared`. The map holds what has been
            // declared *so far*, so an entity declared later in the
            // subset is absent here -- which is the rule: a reference
            // must not precede its declaration.
            None => {
                if !dtd.incomplete {
                    return Err((
                        offset,
                        "a default value references an entity that is not \
                         declared, or is declared after it",
                    ));
                }
            }
            // `WFC: No External Entity References`.
            Some(EntityValue::External { .. }) => {
                return Err((
                    offset,
                    "a default value may not reference an external entity",
                ));
            }
            // `WFC: Parsed Entity`.
            Some(EntityValue::Unparsed) => {
                return Err((
                    offset,
                    "a default value may not reference an unparsed entity",
                ));
            }
            Some(EntityValue::Internal(inner)) => {
                visiting.push(name);
                check_att_value(inner, offset, dtd, visiting)?;
                let _ = visiting.pop();
            }
        }
    }
    Ok(())
}

/// Whether `name` is one of the five entities every parser predefines.
fn decode_predefined_name(name: &str) -> bool {
    matches!(name, "lt" | "gt" | "amp" | "apos" | "quot")
}

/// Whether `name` is a `Name`, for a reference appearing in a literal.
///
/// Uses the fifth-edition rules regardless of the document's declared
/// edition. The two differ only in which exotic characters may start a
/// name, and a reference whose name is invalid under both -- which is
/// what this exists to catch -- is rejected either way.
fn is_reference_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars.next().is_some_and(crate::parser::is_name_start)
        && chars.all(crate::parser::is_name_char)
}

/// Check an `EntityValue` at the point of declaration.
///
/// Two well-formedness constraints apply here and are easy to miss
/// because the value may never be referenced:
///
/// * every character reference in it must be well-formed — `&#002f;` is
///   not a decimal number;
/// * a parameter-entity reference may not appear in an entity value in
///   the **internal** subset (`WFC: PEs in Internal Subset`). In the
///   external subset it is permitted, so this only applies while the
///   declaration is known to be internal.
fn validate_entity_value(
    text: &str,
    offset: usize,
    external_subset_seen: bool,
) -> Result<(), DtdError> {
    let mut rest = text;
    while let Some(i) = rest.find(['&', '%']) {
        let (kind, tail) = (rest.as_bytes()[i], &rest[i + 1..]);
        if kind == b'%' {
            if !external_subset_seen {
                return Err((
                    offset,
                    "a parameter entity reference is not allowed in an \
                     entity value in the internal subset",
                ));
            }
            // `PEReference ::= '%' Name ';'`, and `EntityValue`
            // excludes `%` from its literal characters. So a `%` that
            // begins no reference is not data:
            // `<!ENTITY e "100%">` is malformed, however innocent it
            // looks. Skipping past the `%` accepted it.
            let Some(semi) = tail.find(';') else {
                return Err((
                    offset,
                    "a `%` in an entity value must begin a parameter \
                     entity reference",
                ));
            };
            if !is_reference_name(&tail[..semi]) {
                return Err((
                    offset,
                    "a `%` in an entity value must begin a parameter \
                     entity reference",
                ));
            }
            rest = &tail[semi + 1..];
            continue;
        }
        let Some(semi) = tail.find(';') else {
            return Err((offset, "unterminated reference in an entity value"));
        };
        let name = &tail[..semi];
        rest = &tail[semi + 1..];
        if let Some(hex) = name.strip_prefix("#x") {
            if hex.is_empty() || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
                return Err((
                    offset,
                    "malformed hexadecimal character reference",
                ));
            }
        } else if let Some(dec) = name.strip_prefix('#') {
            if dec.is_empty() || !dec.chars().all(|c| c.is_ascii_digit()) {
                return Err((offset, "malformed decimal character reference"));
            }
        } else if !is_reference_name(name) {
            // Not a character reference, so it is an entity reference
            // and its name must be a `Name`. `&49;` is neither: it has
            // no `#`, and `49` cannot start a name. Nothing checked
            // this, so it read as a reference to an entity that could
            // not exist.
            return Err((offset, "not a valid entity reference name"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ErrorKind, Limits};
    use alloc::format;

    /// Parse `doc` as a doctype under `limits`, keeping the reason.
    fn doctype(doc: &str, limits: &Limits) -> Result<Dtd, DtdError> {
        DtdParser::with_limits(doc, 0, limits).parse_doctype()
    }

    #[test]
    fn each_limit_reason_maps_to_the_kind_that_names_the_bound() {
        assert_eq!(error_kind(LIMIT_NAME), ErrorKind::NameTooLong);
        assert_eq!(error_kind(LIMIT_ATTRIBUTE), ErrorKind::AttributeTooLarge);
        assert_eq!(error_kind(LIMIT_PE), ErrorKind::EntityLimitExceeded);
        assert_eq!(error_kind(LIMIT_DTD), ErrorKind::DtdTooLarge);
        assert_eq!(
            error_kind("expected a name"),
            ErrorKind::MalformedDtd("expected a name")
        );
    }

    #[test]
    fn with_limits_threads_the_name_bound_into_declarations() {
        let doc = format!("<!DOCTYPE d [<!ELEMENT {} ANY>]>", "n".repeat(300));
        assert!(doctype(&doc, &Limits::default()).is_ok());
        let err = doctype(&doc, &Limits::strict()).expect_err("256 cap");
        assert_eq!(
            err,
            (23, LIMIT_NAME),
            "strict() bounds names at 256 and reports the name's start"
        );
    }

    #[test]
    fn new_applies_the_defaults_with_the_given_edition() {
        let limits = crate::Limits {
            edition: crate::Edition::Fourth,
            ..crate::Limits::default()
        };
        let p = DtdParser::with_limits("", 0, &limits);
        assert_eq!(p.limits.edition, crate::Edition::Fourth);
        assert_eq!(p.limits.max_name_length, Limits::default().max_name_length);
    }

    #[test]
    fn an_attribute_default_is_bounded_by_the_attribute_size() {
        let doc = format!(
            "<!DOCTYPE d [<!ATTLIST d a CDATA \"{}\">]>",
            "v".repeat(9_000)
        );
        assert!(doctype(&doc, &Limits::default()).is_ok());
        let err = doctype(&doc, &Limits::strict()).expect_err("8 KiB cap");
        assert_eq!(err.1, LIMIT_ATTRIBUTE);
    }

    #[test]
    fn a_parameter_entity_referenced_between_declarations_is_charged() {
        let doc = "<!DOCTYPE d SYSTEM \"s\" [\
                   <!ENTITY % a \"<!-- forty bytes of comment text here -->\">\
                   %a;%a;%a;]>";
        let mut limits = Limits {
            max_pe_expansion: 100,
            ..Limits::default()
        };
        let err = doctype(doc, &limits).expect_err("three references");
        assert_eq!(err.1, LIMIT_PE);
        limits.max_pe_expansion = 200;
        assert!(doctype(doc, &limits).is_ok());
    }

    #[test]
    fn a_parameter_entity_inside_a_declaration_is_charged() {
        // Legal only in the external subset, which is where expansion
        // inside a declaration happens.
        let subset = "<!ENTITY % m \"(#PCDATA)\">\
                      <!ELEMENT a %m;><!ELEMENT b %m;><!ELEMENT c %m;>";
        let limits = Limits {
            max_pe_expansion: 20,
            ..Limits::default()
        };
        let mut dtd = Dtd::default();
        let err = DtdParser::with_limits(subset, 0, &limits)
            .parse_external_subset(&mut dtd)
            .expect_err("three references of nine bytes");
        assert_eq!(err.1, LIMIT_PE);
        assert_eq!(dtd.pe_spent, 27, "charged before refusing");
    }

    #[test]
    fn the_doctype_budget_counts_declaration_text_once() {
        let subset = "<!ELEMENT a ANY>".repeat(10);
        let mut limits = Limits {
            max_dtd_bytes: Some(subset.len()),
            ..Limits::default()
        };
        let mut dtd = Dtd::default();
        DtdParser::with_limits(&subset, 0, &limits)
            .parse_external_subset(&mut dtd)
            .expect("exactly the budget");
        assert_eq!(dtd.bytes_seen, subset.len());

        limits.max_dtd_bytes = Some(subset.len() - 1);
        let err = DtdParser::with_limits(&subset, 0, &limits)
            .parse_external_subset(&mut Dtd::default())
            .expect_err("one byte over");
        assert_eq!(err.1, LIMIT_DTD);
    }

    #[test]
    fn an_external_parameter_entity_counts_against_the_doctype_budget() {
        struct Big;
        impl crate::external::ExternalSource for Big {
            fn fetch(&self, _: &str, _: Option<&str>) -> Option<&str> {
                Some("<!-- an external parameter entity's content -->")
            }
        }
        let doc = "<!DOCTYPE d [<!ENTITY % e SYSTEM \"e\">]>";
        let limits = Limits {
            max_dtd_bytes: Some(32),
            ..Limits::default()
        };
        let err = DtdParser::with_limits(doc, 0, &limits)
            .with_external(&Big, crate::parser::Version::V10)
            .parse_doctype()
            .expect_err("fetched content is charged before it is held");
        assert_eq!(err.1, LIMIT_DTD);
    }

    #[test]
    fn expand_pes_charges_every_reference_it_resolves() {
        let mut dtd = Dtd::default();
        let _ = dtd.parameters.insert("a".into(), "xxxxxxxxxx".into());
        let _ = dtd.parameters.insert("b".into(), "%a;%a;%a;".into());
        let mut budget = PeBudget {
            spent: 0,
            max: usize::MAX,
            at: 0,
        };
        let out = expand_pes("<!ELEMENT e (%b;)>", &dtd, 0, &mut budget)
            .expect("within budget")
            .expect("all known");
        assert_eq!(out, format!("<!ELEMENT e ({})>", "x".repeat(30)));
        // `%b;` is nine bytes, then three `%a;` of ten each.
        assert_eq!(budget.spent, 9 + 30);

        let mut budget = PeBudget {
            spent: 0,
            max: 20,
            at: 7,
        };
        assert_eq!(
            expand_pes("<!ELEMENT e (%b;)>", &dtd, 0, &mut budget),
            Err((7, LIMIT_PE))
        );
    }

    #[test]
    fn a_percent_inside_a_literal_is_data() {
        let dtd = Dtd::default();
        let mut budget = PeBudget {
            spent: 0,
            max: 0,
            at: 0,
        };
        assert_eq!(
            expand_pes("<!ENTITY x \"50%\">", &dtd, 0, &mut budget),
            Ok(Some("<!ENTITY x \"50%\">".to_owned()))
        );
    }
}
