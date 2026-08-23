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

/// What a general entity expands to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum EntityValue {
    /// A literal replacement text from the internal subset.
    Internal(String),
    /// An external or unparsed entity. Its replacement text is not
    /// available — `oxml` never fetches external resources — but the
    /// *declaration* is, which is enough to tell "undeclared" from
    /// "declared but not retrievable".
    External,
}

/// Declarations gathered from a document type declaration.
#[derive(Debug, Default, Clone)]
pub(crate) struct Dtd {
    /// General entities, by name, in declaration order of first
    /// definition. A later duplicate declaration is ignored, as the
    /// specification requires.
    pub(crate) general: BTreeMap<String, EntityValue>,
    /// Whether an external subset or a parameter entity was referenced.
    ///
    /// When either is true the internal subset is not the whole story,
    /// so an entity we cannot find may well have been declared
    /// somewhere we did not read. Reporting it as undeclared would
    /// reject valid documents, so entity checking relaxes.
    pub(crate) incomplete: bool,
}

impl Dtd {
    /// The replacement text of a general entity, if it has one.
    pub(crate) fn entity(&self, name: &str) -> Option<&EntityValue> {
        self.general.get(name)
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
    /// Name rules differ between XML 1.0 editions, and names appear in
    /// the DTD too — element and attribute declarations, notation
    /// names, and processing-instruction targets.
    pub(crate) edition: crate::Edition,
}

/// What went wrong, as an offset and a static reason.
pub(crate) type DtdError = (usize, &'static str);

impl<'a> DtdParser<'a> {
    pub(crate) const fn new(
        input: &'a str,
        pos: usize,
        edition: crate::Edition,
    ) -> Self {
        Self {
            input,
            bytes: input.as_bytes(),
            pos,
            edition,
        }
    }

    fn is_name_start(&self, c: char) -> bool {
        match self.edition {
            crate::Edition::Fourth => crate::names4e::is_name_start_4e(c),
            _ => crate::parser::is_name_start(c),
        }
    }

    fn is_name_char(&self, c: char) -> bool {
        match self.edition {
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
        let rest = &self.input[self.pos..];
        let mut chars = rest.char_indices();
        match chars.next() {
            Some((_, c)) if self.is_name_start(c) => {}
            _ => return Err((start, "expected a name")),
        }
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if !self.is_name_char(c) {
                end = i;
                break;
            }
        }
        self.pos = start + end;
        Ok(&rest[..end])
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
            let public = self.starts_with("PUBLIC");
            self.pos += 6;
            self.require_ws()?;
            let _ = self.quoted()?;
            if public {
                self.skip_ws();
                if matches!(self.peek(), Some(b'"' | b'\'')) {
                    let _ = self.quoted()?;
                }
            }
            dtd.incomplete = true;
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

    fn parse_internal_subset(&mut self, dtd: &mut Dtd) -> Result<(), DtdError> {
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err((self.pos, "unterminated internal subset")),
                Some(b']') => return Ok(()),
                // A parameter-entity reference can expand to anything,
                // including further declarations, so once one appears
                // the subset is no longer fully known to us.
                Some(b'%') => {
                    self.pos += 1;
                    let _ = self.name()?;
                    self.expect(
                        b';',
                        "unterminated parameter entity reference",
                    )?;
                    dtd.incomplete = true;
                }
                Some(b'<') => self.parse_markup_decl(dtd)?,
                _ => return Err((self.pos, "expected a markup declaration")),
            }
        }
    }

    fn parse_markup_decl(&mut self, dtd: &mut Dtd) -> Result<(), DtdError> {
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
            return match self.input[self.pos..].find("]]>") {
                Some(i) => {
                    self.pos += i + 3;
                    Ok(())
                }
                None => Err((self.pos, "unterminated conditional section")),
            };
        }
        if self.starts_with("<!ELEMENT") {
            self.pos += "<!ELEMENT".len();
            return self.parse_element_decl();
        }
        if self.starts_with("<!ATTLIST") {
            self.pos += "<!ATTLIST".len();
            return self.parse_attlist_decl();
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

    fn skip_balanced_parens(&mut self) -> Result<(), DtdError> {
        let start = self.pos;
        let mut depth = 0usize;
        while let Some(b) = self.peek() {
            match b {
                b'(' => depth += 1,
                b')' => {
                    depth -= 1;
                    if depth == 0 {
                        self.pos += 1;
                        // An occurrence indicator may follow.
                        if matches!(self.peek(), Some(b'?' | b'*' | b'+')) {
                            self.pos += 1;
                        }
                        return Ok(());
                    }
                }
                b'>' if depth == 0 => break,
                _ => {}
            }
            self.pos += 1;
        }
        Err((start, "unbalanced content model"))
    }

    fn parse_attlist_decl(&mut self) -> Result<(), DtdError> {
        self.require_ws()?;
        let _ = self.name()?;
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
            let _ = self.name()?;
            self.require_ws()?;
            self.parse_att_type()?;
            self.require_ws()?;
            let _ = self.parse_default_decl()?;
        }
    }

    fn parse_att_type(&mut self) -> Result<(), DtdError> {
        if self.peek() == Some(b'(') {
            return self.skip_balanced_parens();
        }
        let kw = self.name()?;
        match kw {
            "CDATA" | "ID" | "IDREF" | "IDREFS" | "ENTITY" | "ENTITIES"
            | "NMTOKEN" | "NMTOKENS" => Ok(()),
            "NOTATION" => {
                self.require_ws()?;
                if self.peek() != Some(b'(') {
                    return Err((self.pos, "NOTATION needs a name list"));
                }
                self.skip_balanced_parens()
            }
            _ => Err((self.pos, "unknown attribute type")),
        }
    }

    fn parse_default_decl(&mut self) -> Result<DefaultDecl, DtdError> {
        if self.peek() == Some(b'#') {
            self.pos += 1;
            let kw = self.name()?;
            return match kw {
                "REQUIRED" => Ok(DefaultDecl::Required),
                "IMPLIED" => Ok(DefaultDecl::Implied),
                "FIXED" => {
                    self.require_ws()?;
                    let _ = self.quoted()?;
                    Ok(DefaultDecl::Fixed)
                }
                _ => Err((self.pos, "expected #REQUIRED, #IMPLIED or #FIXED")),
            };
        }
        let _ = self.quoted()?;
        Ok(DefaultDecl::Value)
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
        let name = self.name()?;
        self.require_ws()?;

        let value = if matches!(self.peek(), Some(b'"' | b'\'')) {
            EntityValue::Internal(self.quoted()?.to_owned())
        } else {
            let public = self.starts_with("PUBLIC");
            if !public && !self.starts_with("SYSTEM") {
                return Err((
                    self.pos,
                    "expected an entity value or ExternalID",
                ));
            }
            self.pos += 6;
            self.require_ws()?;
            let _ = self.quoted()?;
            if public {
                self.require_ws()?;
                let _ = self.quoted()?;
            }
            // NDataDecl, for an unparsed entity.
            self.skip_ws();
            if self.starts_with("NDATA") {
                self.pos += "NDATA".len();
                self.require_ws()?;
                let _ = self.name()?;
            }
            EntityValue::External
        };

        self.skip_ws();
        self.expect(b'>', "unterminated entity declaration")?;

        if parameter {
            // The replacement text of a parameter entity can contain
            // declarations we have not expanded, so treat its presence
            // as making the subset incomplete.
            dtd.incomplete = true;
        } else {
            // "the first declaration binds" — a later duplicate is not
            // an error, it is ignored.
            let _ = dtd.general.entry(name.to_owned()).or_insert(value);
        }
        Ok(())
    }

    fn parse_notation_decl(&mut self) -> Result<(), DtdError> {
        self.require_ws()?;
        let _ = self.name()?;
        self.require_ws()?;
        let public = self.starts_with("PUBLIC");
        if !public && !self.starts_with("SYSTEM") {
            return Err((self.pos, "notation needs SYSTEM or PUBLIC"));
        }
        self.pos += 6;
        self.require_ws()?;
        let _ = self.quoted()?;
        self.skip_ws();
        if public && matches!(self.peek(), Some(b'"' | b'\'')) {
            let _ = self.quoted()?;
            self.skip_ws();
        }
        self.expect(b'>', "unterminated notation declaration")
    }
}
