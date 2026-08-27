// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The parser: XML text in, [`Document`] out.
//!
//! A single forward pass over the input, byte-oriented on the hot
//! paths and `char`-aware only where XML actually requires it (name
//! characters). No backtracking, no intermediate token vector — the
//! tree is built directly as the scan proceeds.

use alloc::borrow::{Cow, ToOwned};
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, ErrorKind, Result};
use crate::limits::Limits;
use crate::tree::{Chars, Document, ExpandedName, NameId, NodeData, NodeId};

/// Namespace bindings in scope, as a stack of (prefix, uri) frames.
///
/// A `Vec` searched backwards rather than a map: scopes are tiny in
/// practice (a handful of bindings), and this keeps push/pop free of
/// allocation and hashing on the hot path.
#[derive(Debug, Default, Clone)]
pub(crate) struct Namespaces {
    bindings: Vec<(String, String)>,
    marks: Vec<usize>,
}

impl Namespaces {
    fn push_scope(&mut self) {
        self.marks.push(self.bindings.len());
    }

    pub(crate) fn pop_scope(&mut self) {
        if let Some(mark) = self.marks.pop() {
            self.bindings.truncate(mark);
        }
    }

    fn bind(&mut self, prefix: String, uri: String) {
        self.bindings.push((prefix, uri));
    }

    fn resolve(&self, prefix: &str) -> Option<&str> {
        self.bindings
            .iter()
            .rev()
            .find(|(p, _)| p == prefix)
            .map(|(_, u)| u.as_str())
    }
}

pub(crate) struct Parser<'a> {
    pub(crate) input: &'a str,
    pub(crate) bytes: &'a [u8],
    pub(crate) pos: usize,
    pub(crate) doc: Document,
    pub(crate) ns: Namespaces,
    /// How many elements are currently open. Bounded by
    /// [`Limits::max_depth`] so the recursion cannot exhaust the stack.
    pub(crate) depth: usize,
    /// Resource bounds for this parse.
    pub(crate) limits: Limits,
    /// Declarations from the document type declaration, once seen.
    pub(crate) dtd: Option<crate::dtd::Dtd>,
    /// The version the XML declaration names.
    ///
    /// 1.1 differs from 1.0 in three ways that matter here: NEL and
    /// LINE SEPARATOR normalise to LF, C1 controls must be escaped
    /// rather than appearing literally, and the `Char` production
    /// admits C0 controls when written as character references.
    pub(crate) version: Version,
    /// Element names seen so far, keyed on the local part.
    ///
    /// Keyed on the local part rather than the whole name so that a
    /// document with many distinct names does not degrade to a linear
    /// scan of the table; the few sharing a local part are compared on
    /// their namespace.
    pub(crate) name_index: alloc::collections::BTreeMap<String, Vec<u32>>,
    /// Where external content comes from, if anywhere.
    ///
    /// Never a file or a socket: the caller supplies it, so the parser
    /// performs no I/O whatever this is.
    pub(crate) external: &'a dyn crate::external::ExternalSource,
    /// Characters of entity expansion still permitted **for the whole
    /// document**.
    ///
    /// Per-document rather than per-reference. A per-reference budget
    /// bounds the exponential billion-laughs shape but not the
    /// quadratic one: referencing a single 100 KB entity a thousand
    /// times at depth one produced 100 MB from 100 KB of input while
    /// every individual expansion stayed within its allowance.
    pub(crate) entity_budget: usize,
    /// How many entity replacement texts are being parsed above this
    /// one, so a self-referential entity terminates.
    pub(crate) entity_depth: usize,
}

/// Parse an XML document.
///
/// # Errors
///
/// Returns [`Error`] if the input is not well-formed, or uses a
/// namespace prefix that was never declared.
pub fn parse(input: &str) -> Result<Document> {
    parse_with(input, Limits::default())
}

/// Parse an XML document from bytes.
///
/// Prefer [`parse`] when you already have a `&str`: it borrows from the
/// input, while decoding a non-UTF-8 encoding must allocate.
///
/// # Errors
///
/// As [`parse`], and additionally if the bytes are not valid in their
/// declared encoding.
pub fn parse_bytes(input: &[u8]) -> Result<Document> {
    parse_bytes_with(input, Limits::default())
}

/// Parse from bytes, decoding the encoding the document declares,
/// under explicit resource bounds.
///
/// A byte-order mark wins, then the `encoding` pseudo-attribute of the
/// XML declaration, then UTF-8. UTF-8 input is not transcoded, so the
/// zero-copy path is preserved for the common case.
///
/// # Errors
///
/// As [`parse_with`], and additionally if the bytes are not valid in
/// their declared encoding or that encoding is not supported.
pub fn parse_bytes_with(input: &[u8], limits: Limits) -> Result<Document> {
    let text = crate::encoding::decode(input)?;
    parse_with(&text, limits)
}

/// Parse an XML document under explicit resource bounds.
///
/// Use this for input from an untrusted source, where the defaults may
/// be more generous than you want. See [`Limits`].
///
/// # Errors
///
/// Returns [`Error`] if the input is not well-formed, uses an
/// undeclared namespace prefix, contains a character the `Char`
/// production forbids, or exceeds one of `limits`. Each bound has its
/// own [`ErrorKind`], so a caller can tell which one was hit.
pub fn parse_with(input: &str, limits: Limits) -> Result<Document> {
    parse_with_external(input, limits, &crate::external::NoExternal)
}

/// Parse with external content the caller supplies.
///
/// oxml never performs I/O, so a document that names an external entity
/// or subset gets nothing unless the caller provides it. With a source,
/// the same parse can also check the rules only the external content
/// can settle -- chiefly that a text declaration is well formed, names
/// an encoding, and does not claim a version later than the document's.
///
/// # Errors
///
/// Returns [`Error`] if the input is not well-formed, or if content the
/// source supplies is not.
pub fn parse_with_external(
    input: &str,
    limits: Limits,
    external: &dyn crate::external::ExternalSource,
) -> Result<Document> {
    // End-of-line normalisation happens before anything else, because
    // the specification defines it as something the processor behaves
    // as though it had done "on input, before parsing". Doing it later
    // means every rule that inspects whitespace has to know about it,
    // and the thirteenth such rule is the one that forgets.
    let version = declared_version(input)?;
    // The document owns its text, because text nodes, comments and
    // attribute values are ranges into it rather than strings of their
    // own. That costs one copy of the input for a document whose line
    // endings needed no rewriting -- against one allocation per text
    // node and per attribute value, which is what it replaces.
    //
    // The ranges are recorded against exactly this buffer, and moving
    // a `String` does not move the bytes it points at, so handing it
    // to the document afterwards leaves every range valid.
    let text = normalize_line_endings(input, version).into_owned();
    let mut doc = parse_normalized(&text, limits, version, external)?;
    doc.input = text;
    Ok(doc)
}

/// Everything that happens to a document before any of it is scanned.
///
/// The version it declares, its line endings normalised, and the
/// `Char` production checked over the whole input. Shared with
/// [`crate::stream::Reader`] so a streaming caller cannot be given a
/// document the tree parser would have refused.
///
/// Returns owned text because normalisation may rewrite it, and a
/// reader that borrowed the caller's string could not hold the
/// rewritten form.
///
/// # Errors
///
/// Returns [`Error`] if the declaration is malformed or a character is
/// one XML forbids.
pub(crate) fn prepare(
    input: &str,
    limits: Limits,
) -> Result<(String, Version)> {
    let version = declared_version(input)?;
    let _ = limits;
    let text = normalize_line_endings(input, version).into_owned();
    check_prolog_shape(&text)?;
    if let Some((offset, c)) = text
        .char_indices()
        .find(|(_, c)| !is_literal_char_for(*c, version))
    {
        return Err(Error::new(ErrorKind::IllegalCharacter(c), offset));
    }
    Ok((text, version))
}

/// A character-data run being accumulated.
///
/// Almost every text node in almost every document is exactly a slice
/// of the input: no references, no `CDATA`, nothing merged. That case
/// must not allocate, so a run remembers where it started and only
/// materialises a `String` when something forces it to -- an entity
/// expanded, or a second piece that is not contiguous with the first.
#[derive(Debug, Default)]
pub(crate) struct Run {
    /// Where the verbatim part starts in the input.
    start: usize,
    /// How much of the input it covers.
    len: usize,
    /// Set once the run stopped being expressible as a slice.
    owned: Option<String>,
}

impl Run {
    /// Add a piece that *is* a slice of the input, at `at`.
    pub(crate) fn push_slice(&mut self, at: usize, text: &str, input: &str) {
        if let Some(owned) = &mut self.owned {
            owned.push_str(text);
        } else if self.len == 0 {
            self.start = at;
            self.len = text.len();
        } else if self.start + self.len == at {
            // Contiguous with what is already here, so the two are one
            // slice and the run is still free.
            self.len += text.len();
        } else {
            // A gap -- a `CDATA` delimiter, most likely. From here the
            // run is no longer a range into anything.
            self.push_owned(text, input);
        }
    }

    /// Add a piece the input does not contain, such as an expansion.
    pub(crate) fn push_owned(&mut self, text: &str, input: &str) {
        if let Some(owned) = &mut self.owned {
            owned.push_str(text);
            return;
        }
        let mut buffer = String::with_capacity(self.len + text.len());
        buffer.push_str(
            input
                .get(self.start..self.start + self.len)
                .unwrap_or_default(),
        );
        buffer.push_str(text);
        self.owned = Some(buffer);
    }

    /// The run's text, borrowed from wherever it lives.
    pub(crate) fn as_str<'a>(&'a self, input: &'a str) -> &'a str {
        self.owned.as_deref().unwrap_or_else(|| {
            input
                .get(self.start..self.start + self.len)
                .unwrap_or_default()
        })
    }

    /// How many bytes the run holds, for the length limit.
    pub(crate) fn len(&self) -> usize {
        self.owned.as_ref().map_or(self.len, String::len)
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Where the run sits in the input, if it is still a slice.
    pub(crate) fn as_span(&self) -> Option<(usize, usize)> {
        if self.owned.is_some() {
            None
        } else {
            Some((self.start, self.len))
        }
    }
}

/// Whitespace before an XML declaration is a malformed document, not
/// `Misc` appearing early.
fn check_prolog_shape(input: &str) -> Result<()> {
    let trimmed = input.trim_start();
    if trimmed.len() != input.len() && trimmed.starts_with("<?xml") {
        let after = &trimmed["<?xml".len()..];
        // `<?xmlfoo?>` is a processing instruction with a reserved
        // target, reported elsewhere; only a real declaration counts.
        if after.starts_with([' ', '\t', '\r', '\n']) {
            return Err(Error::new(ErrorKind::MalformedDeclaration, 0));
        }
    }
    Ok(())
}

/// Translate line endings to `\n`, per XML 1.0 and 1.1 section 2.11.
///
/// XML 1.0 collapses `\r\n` and a lone `\r`. XML 1.1 adds NEL
/// (U+0085) and LINE SEPARATOR (U+2028), and treats `\r` followed by
/// NEL as one ending rather than two.
///
/// Borrows when there is nothing to change, which is the common case:
/// a document written on a Unix-like system contains no carriage
/// return at all and pays only for the scan.
///
/// Character references are untouched, which is what the
/// specification requires. `&#xD;` is still markup at this point and
/// becomes a carriage return when the reference is expanded -- it is
/// the only way to write one that survives.
/// Append `text` to an attribute value, normalising whitespace.
///
/// XML section 3.3.3 requires each whitespace character in an
/// attribute value to be replaced by a space. Line endings are already
/// `\n` by the time this runs, so in practice this converts `\n` and
/// `\t`; `\r` only reaches here from a character reference, which is
/// exempt and never passed to this function.
///
/// Without it, an attribute written across two lines came back
/// containing the newline and the indentation of the next line -- so
/// `title="a\n    b"` rather than `title="a     b"`, and two documents
/// that differ only in line wrapping produced different values.
/// The offset of the next `&` that is a reference rather than text.
///
/// Skips over CDATA sections, inside which `&` introduces nothing.
/// What a start tag yields, before anything is done with it.
pub(crate) struct StartTag<'a> {
    /// The element's name, as written.
    pub(crate) qname: &'a str,
    /// Attributes, as written, with entities resolved in the values.
    pub(crate) raw_attrs: Vec<(&'a str, Run)>,
    /// Namespaces this tag declares, as `(prefix, uri)`.
    pub(crate) declared: Vec<(String, String)>,
    /// Whether the tag closed itself.
    pub(crate) self_closing: bool,
    /// Where the tag began, for an error offset.
    pub(crate) tag_start: usize,
}

fn next_reference(text: &str) -> Option<usize> {
    let mut at = 0;
    while at < text.len() {
        let rest = &text[at..];
        let amp = rest.find('&');
        let cdata = rest.find("<![CDATA[");
        match (amp, cdata) {
            (Some(a), Some(c)) if c < a => {
                // Skip the whole section; an unterminated one is left
                // for the parser proper to report.
                let after = at + c + "<![CDATA[".len();
                let end = text[after..].find("]]>")?;
                at = after + end + "]]>".len();
            }
            (Some(a), _) => return Some(at + a),
            (None, _) => return None,
        }
    }
    None
}

fn push_attribute_normalized(out: &mut String, text: &str) {
    // The overwhelmingly common case is a value with no whitespace to
    // replace, which copies in one go.
    if !text.contains(['\n', '\t', '\r']) {
        out.push_str(text);
        return;
    }
    for c in text.chars() {
        out.push(match c {
            '\n' | '\t' | '\r' => ' ',
            other => other,
        });
    }
}

pub(crate) fn normalize_line_endings(
    input: &str,
    version: Version,
) -> Cow<'_, str> {
    let terminator = |c: char| {
        c == '\r'
            || (version == Version::V11 && (c == '\u{85}' || c == '\u{2028}'))
    };
    if !input.chars().any(terminator) {
        return Cow::Borrowed(input);
    }

    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                let paired = matches!(chars.peek(), Some('\n'))
                    || (version == Version::V11
                        && matches!(chars.peek(), Some('\u{85}')));
                if paired {
                    // The pair is one line ending; drop the second half.
                    let _ = chars.next();
                }
                out.push('\n');
            }
            '\u{85}' | '\u{2028}' if version == Version::V11 => {
                out.push('\n');
            }
            other => out.push(other),
        }
    }
    Cow::Owned(out)
}

/// Parse a document whose line endings are already normalised.
fn parse_normalized(
    input: &str,
    limits: Limits,
    version: Version,
    external: &dyn crate::external::ExternalSource,
) -> Result<Document> {
    // `document ::= prolog element Misc*` with
    // `prolog ::= XMLDecl? Misc*` -- the declaration comes first or not
    // at all. Whitespace before it is not `Misc` appearing early, it is
    // a malformed document, and skipping leading whitespace before
    // looking for `<?xml` accepted it.
    let trimmed = input.trim_start();
    if trimmed.len() != input.len() && trimmed.starts_with("<?xml") {
        let after = &trimmed["<?xml".len()..];
        // `<?xmlfoo?>` is a processing instruction with a reserved
        // target, reported elsewhere; only a real declaration counts.
        if after.starts_with([' ', '\t', '\r', '\n']) {
            return Err(Error::new(ErrorKind::MalformedDeclaration, 0));
        }
    }
    // Checked once over the whole input rather than at each construct.
    // The `Char` production applies everywhere — content, attribute
    // values, comments, even the DTD — so a per-construct check would
    // have to be repeated in a dozen places and would be forgotten in
    // the thirteenth. One pass is also faster than one branch per
    // character in the hot loops.
    if let Some((offset, c)) = input
        .char_indices()
        .find(|(_, c)| !is_literal_char_for(*c, version))
    {
        return Err(Error::new(ErrorKind::IllegalCharacter(c), offset));
    }

    let mut p = Parser {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        // One byte-scan to size the arena. Every element, comment and
        // processing instruction begins with `<`, and text nodes are
        // bounded by them. Counting runs at memory speed; the
        // reallocate-and-copy it avoids does not.
        doc: Document::with_capacity(
            input.bytes().filter(|b| *b == b'<').count() * 2,
        ),
        name_index: alloc::collections::BTreeMap::new(),
        external,
        ns: Namespaces::default(),
        depth: 0,
        limits,
        dtd: None,
        version,
        entity_budget: limits.max_entity_expansion,
        entity_depth: 0,
    };
    p.parse_document()?;
    Ok(p.doc)
}

impl<'a> Parser<'a> {
    fn parse_document(&mut self) -> Result<()> {
        let root = self.doc.root();
        let mark = self.doc.scratch_mark();
        self.skip_prolog()?;

        let mut seen_root = false;
        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() {
                break;
            }
            if self.peek_is(b'<') {
                if self.starts_with("<!--") {
                    let c = self.parse_comment()?;
                    let _ = self.doc.push(NodeData::Comment(c), root);
                } else if self.starts_with("<?") {
                    let (t, d) = self.parse_pi()?;
                    let _ = self.doc.push(
                        NodeData::ProcessingInstruction { target: t, data: d },
                        root,
                    );
                } else if self.starts_with("<!DOCTYPE") {
                    // Handled here as well as in `skip_prolog`, because
                    // the prolog is `Misc* doctypedecl? Misc*` — a
                    // comment or PI may precede the declaration. The
                    // prolog loop breaks on anything that is not a
                    // DOCTYPE, so a leading comment left the DOCTYPE to
                    // be parsed as an element and reported as
                    // `expected a name`.
                    if seen_root || self.dtd.is_some() {
                        return Err(Error::new(
                            ErrorKind::TrailingContent,
                            self.pos,
                        ));
                    }
                    self.skip_doctype()?;
                } else if self.starts_with("</") {
                    let name = self.peek_end_tag_name();
                    return Err(Error::new(
                        ErrorKind::UnexpectedEndTag(name),
                        self.pos,
                    ));
                } else {
                    if seen_root {
                        return Err(Error::new(
                            ErrorKind::TrailingContent,
                            self.pos,
                        ));
                    }
                    self.parse_element(root)?;
                    seen_root = true;
                }
            } else {
                // Non-whitespace character data outside the root
                // element is never well-formed.
                return Err(Error::new(ErrorKind::TrailingContent, self.pos));
            }
        }

        if seen_root {
            self.doc.finish_children(root, mark);
            Ok(())
        } else {
            Err(Error::new(ErrorKind::NoRootElement, self.pos))
        }
    }

    /// Skip the XML declaration, doctype, and any leading misc.
    ///
    /// The declaration is consumed rather than modelled: nothing in the
    /// tree depends on it, and preserving it would mean a node kind
    /// that `XPath` has no concept of.
    pub(crate) fn skip_prolog(&mut self) -> Result<()> {
        self.skip_whitespace();
        if self.starts_with("<?xml")
            && !matches!(
                self.bytes.get(self.pos + 5),
                Some(c) if is_name_char(char::from(*c))
            )
        {
            let end = self.input[self.pos..].find("?>").ok_or_else(|| {
                Error::new(ErrorKind::Unterminated("XML declaration"), self.pos)
            })?;
            let decl = &self.input[self.pos + 5..self.pos + end];
            validate_xml_declaration(decl, self.pos, true)?;
            self.pos += end + 2;
        }
        loop {
            self.skip_whitespace();
            if self.starts_with("<!DOCTYPE") {
                self.skip_doctype()?;
            } else {
                break;
            }
        }
        Ok(())
    }

    /// Skip a doctype, tracking bracket depth so an internal subset
    /// containing `>` does not end it early.
    pub(crate) fn skip_doctype(&mut self) -> Result<()> {
        // Parsed rather than skipped. Well-formedness constraints live
        // inside the DTD — a malformed `<!ATTLIST>` makes a document not
        // well-formed for *every* parser, validating or not — and the
        // general entity declarations are needed so that a document
        // using `&chapter1;` is not rejected as undeclared.
        let mut p = crate::dtd::DtdParser::new(
            self.input,
            self.pos,
            self.limits.edition,
        )
        // So an external parameter entity the caller supplied can be
        // read. Without this the DTD parser sees no source at all and
        // `%pExternal;` pulls in nothing.
        .with_external(self.external, self.version);
        match p.parse_doctype() {
            Ok(mut dtd) => {
                self.pos = p.pos;
                // The external subset, if the caller can supply it.
                // Parsed *after* the internal one, because the internal
                // subset takes precedence: "the first declaration
                // binds", and `or_insert` in the entity map already
                // implements that.
                if let Some((system, public)) = dtd.external_subset.clone() {
                    if let Some(content) =
                        self.external.fetch(&system, public.as_deref())
                    {
                        let version = entity_version(content, self.version);
                        let normalized =
                            normalize_line_endings(content, version);
                        check_text_decl_position(&normalized, self.pos)?;
                        check_text_decl(&normalized, self.version, self.pos)?;
                        let body = strip_text_decl(&normalized);
                        // Judged by **both** versions, and it has to
                        // be both.
                        //
                        // The subset's own version rules it: a DTD
                        // declaring 1.0 may not contain characters only
                        // 1.1 allows, which is what carrying a version
                        // per entity is for. But the including
                        // document's version rules it too. XML 1.1
                        // section 4.3.4: a 1.0 external DTD may hold
                        // `#x7F`, legal in 1.0, and pulling it into a
                        // document declaring 1.1 puts a character there
                        // that 1.1 requires to be escaped. Checking
                        // only the entity's version accepted seven
                        // documents the suite calls not well-formed.
                        if let Some((_, c)) =
                            body.char_indices().find(|(_, c)| {
                                !is_literal_char_for(*c, version)
                                    || !is_literal_char_for(*c, self.version)
                            })
                        {
                            return Err(Error::new(
                                ErrorKind::IllegalCharacter(c),
                                self.pos,
                            ));
                        }
                        let mut sub = crate::dtd::DtdParser::new(
                            body,
                            0,
                            self.limits.edition,
                        )
                        .with_external(self.external, version);
                        if let Err((offset, reason)) =
                            sub.parse_external_subset(&mut dtd)
                        {
                            let _ = offset;
                            return Err(Error::new(
                                ErrorKind::MalformedDtd(reason),
                                self.pos,
                            ));
                        }
                        // The declarations are now known, so an entity
                        // that is still missing really is undeclared.
                        dtd.incomplete = false;
                    }
                }
                // An external entity is checked when it is
                // *referenced*, not when it is declared. A processor
                // need not read an entity nothing uses, so validating
                // eagerly rejected a valid document whose unreferenced
                // entity had no `encoding` -- a rule that only applies
                // to content you actually read.
                self.dtd = Some(dtd);
                Ok(())
            }
            Err((offset, reason)) => {
                Err(Error::new(ErrorKind::MalformedDtd(reason), offset))
            }
        }
    }

    fn parse_element(&mut self, parent: NodeId) -> Result<()> {
        // Checked on entry so the frame that would overflow is never
        // pushed. See `crate::MAX_DEPTH`.
        if self.depth >= self.limits.max_depth {
            return Err(Error::new(ErrorKind::DepthLimitExceeded, self.pos));
        }
        self.depth += 1;
        let result = self.parse_element_inner(parent);
        self.depth -= 1;
        result
    }

    /// Apply the `xmlns` declarations on a start tag.
    ///
    /// Split out of `parse_element_inner` because it is where the
    /// Namespaces specification's rules live, and it had grown twice --
    /// once for prefixed reserved URIs, once for the default
    /// declaration, which had been missed.
    /// Bind the `xmlns` declarations on a start tag, and report them
    /// as `(prefix, uri)` pairs in source order.
    ///
    /// The pairs become namespace nodes. An *undeclaration* -- an
    /// empty URI, which is XML 1.1 only -- is reported too, even
    /// though it names no namespace: it has to shadow the same prefix
    /// declared on an ancestor, and a prefix that is simply absent
    /// here would leave the ancestor's binding in scope.
    pub(crate) fn bind_namespaces(
        &mut self,
        raw_attrs: &[(&'a str, Run)],
    ) -> Result<Vec<(String, String)>> {
        let mut declared = Vec::new();
        // Copied out so resolving a run does not borrow `self` while
        // the loop binds into `self.ns`.
        let input = self.input;
        for (name, run) in raw_attrs {
            let value = run.as_str(input);
            if let Some(prefix) = name.strip_prefix("xmlns:") {
                // Namespaces in XML, section 3: `xml` may be bound only
                // to the XML namespace and nothing else may be bound to
                // it; `xmlns` may not be declared at all.
                const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
                const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
                if prefix == "xmlns"
                    || (prefix == "xml" && value != XML_NS)
                    || (prefix != "xml" && value == XML_NS)
                    || value == XMLNS_NS
                {
                    return Err(Error::new(
                        ErrorKind::ReservedNamespace,
                        self.pos,
                    ));
                }
                // `xmlns:` with nothing after it: the attribute parses
                // as a name, but `PrefixedAttName ::= 'xmlns:' NCName`
                // needs a prefix to declare.
                if prefix.is_empty() || prefix.contains(':') {
                    return Err(Error::new(
                        ErrorKind::ReservedNamespace,
                        self.pos,
                    ));
                }
                // Undeclaring a prefix -- binding it to the empty
                // string -- is XML 1.1 only. In a 1.0 document it is an
                // error, not a no-op, and treating it as one silently
                // changed which namespace the element was in.
                if value.is_empty() && self.version == Version::V10 {
                    return Err(Error::new(
                        ErrorKind::ReservedNamespace,
                        self.pos,
                    ));
                }
                self.ns.bind(prefix.to_owned(), value.to_owned());
                declared.push((prefix.to_owned(), value.to_owned()));
            } else if *name == "xmlns" {
                // The reserved URIs were checked for *prefixed*
                // declarations and not for the default one, so
                // `xmlns="http://www.w3.org/XML/1998/namespace"` --
                // which the Namespaces specification forbids outright
                // -- went through.
                const XML_NS: &str = "http://www.w3.org/XML/1998/namespace";
                const XMLNS_NS: &str = "http://www.w3.org/2000/xmlns/";
                if value == XML_NS || value == XMLNS_NS {
                    return Err(Error::new(
                        ErrorKind::ReservedNamespace,
                        self.pos,
                    ));
                }
                self.ns.bind(String::new(), value.to_owned());
                declared.push((String::new(), value.to_owned()));
            }
        }
        Ok(declared)
    }

    /// Read a start tag: its name, its attributes, the namespaces it
    /// declares, and whether it closed itself.
    ///
    /// Extracted so the streaming reader runs the *same* scanner. Two
    /// implementations of XML start-tag scanning would diverge, and
    /// the one with fewer users would be the one that was wrong.
    ///
    /// A namespace scope is pushed on success and left for the caller
    /// to pop when the element ends. On failure it is popped here, so
    /// a rejected tag leaves no scope behind.
    pub(crate) fn scan_start_tag(&mut self) -> Result<StartTag<'a>> {
        let tag_start = self.pos;
        self.pos += 1; // '<'
        let qname = self.parse_name()?;

        // Attributes are collected before the element node is created:
        // namespace declarations among them must be in scope for the
        // element's *own* name to resolve.
        self.ns.push_scope();
        let raw_attrs = self.parse_attributes()?;
        let declared = self.bind_namespaces(&raw_attrs)?;
        let self_closing = if self.starts_with("/>") {
            self.pos += 2;
            true
        } else if self.peek_is(b'>') {
            self.pos += 1;
            false
        } else {
            self.ns.pop_scope();
            return Err(Error::new(
                ErrorKind::Unterminated("start tag"),
                tag_start,
            ));
        };
        Ok(StartTag {
            qname,
            raw_attrs,
            declared,
            self_closing,
            tag_start,
        })
    }

    fn parse_element_inner(&mut self, parent: NodeId) -> Result<()> {
        let StartTag {
            qname,
            raw_attrs,
            declared,
            self_closing,
            tag_start,
        } = self.scan_start_tag()?;

        // Resolve attribute names first so duplicates are detected
        // before any node is created — otherwise a rejected element
        // would already be in the arena.
        let mut resolved: Vec<(NameId, Chars)> =
            Vec::with_capacity(raw_attrs.len());
        let input = self.input;
        // Values of this element's `ID`-typed attributes, held until
        // the element node exists to point them at.
        let mut id_values: Vec<String> = Vec::new();
        for (raw, run) in raw_attrs {
            if raw == "xmlns" || raw.starts_with("xmlns:") {
                continue;
            }
            let an = self.intern_qname(raw, false, tag_start)?;
            // Compared by *expanded* name, not by id. Namespaces in
            // XML forbids two attributes with the same expanded name
            // however they are spelled, so `p:a` and `q:a` with `p` and
            // `q` bound to one namespace collide. Ids no longer settle
            // that on their own: they distinguish prefixes, so the two
            // get different ids and an id comparison would let the pair
            // through.
            let expanded = &self.doc.names[an.0 as usize];
            if resolved
                .iter()
                .any(|(n, _)| self.doc.names[n.0 as usize] == *expanded)
            {
                self.ns.pop_scope();
                return Err(Error::new(
                    ErrorKind::DuplicateAttribute(raw.to_owned()),
                    tag_start,
                ));
            }
            if self
                .dtd
                .as_ref()
                .is_some_and(|d| d.is_id_attribute(qname, raw))
            {
                id_values.push(run.as_str(input).to_owned());
            }
            // A value that survived scanning as a slice becomes a
            // range into the document's own input; one that entity
            // expansion or normalisation rewrote joins the side table.
            let value = match run.as_span() {
                Some((start, len)) => self.verbatim(start, len),
                None => self.doc.push_expanded(run.as_str(input)),
            };
            resolved.push((an, value));
        }

        let name_id = self.intern_qname(qname, true, tag_start)?;
        let node = self.doc.push(
            NodeData::Element {
                name: name_id,
                attributes: (0, 0),
                namespaces: (0, 0),
            },
            parent,
        );

        // Attribute nodes are parented to the element but kept out of
        // its `children`, so `child::` cannot see them. They are pushed
        // consecutively, so they are already contiguous in `attr_ids`
        // and need no scratch buffer.
        let start = self.doc.attr_ids.len();
        for (name, value) in resolved {
            let id =
                self.doc.push_detached(NodeData::Attr { name, value }, node);
            self.doc.attr_ids.push(id);
        }
        for value in id_values {
            // Two elements sharing an ID makes the document invalid,
            // which this parser does not enforce. `id()` still has to
            // answer with one node, so the first declaration wins
            // rather than the last silently replacing it.
            let _ = self.doc.ids.entry(value).or_insert(node);
        }

        let len = self.doc.attr_ids.len() - start;

        // Namespace nodes, like attribute nodes: parented to the
        // element, kept out of `children`, contiguous in their own
        // arena. `xml` is bound by specification and never declared,
        // so the root element carries a node for it and every element
        // inherits it by the same ancestor walk as any other prefix.
        let (ns_start, ns_len) = self.push_namespace_nodes(node, declared);

        if let Some(NodeData::Element {
            attributes,
            namespaces,
            ..
        }) = self.doc.data_mut(node)
        {
            *attributes = (
                u32::try_from(start).unwrap_or(u32::MAX),
                u32::try_from(len).unwrap_or(u32::MAX),
            );
            *namespaces = (
                u32::try_from(ns_start).unwrap_or(u32::MAX),
                u32::try_from(ns_len).unwrap_or(u32::MAX),
            );
        }

        if self_closing {
            self.ns.pop_scope();
            return Ok(());
        }

        self.parse_children(node, qname)?;
        self.ns.pop_scope();
        Ok(())
    }

    /// Attach an element's namespace nodes, returning their
    /// `(start, len)` range in the shared arena.
    ///
    /// `xml` is bound by specification and never declared, so the root
    /// element carries a node for it and every element inherits it
    /// through the same ancestor walk as any other prefix -- one node
    /// per document rather than one per element.
    fn push_namespace_nodes(
        &mut self,
        node: NodeId,
        declared: Vec<(String, String)>,
    ) -> (usize, usize) {
        let start = self.doc.ns_ids.len();
        if self.doc.nodes[node.0].parent == Some(self.doc.root()) {
            // Namespace nodes go in the side table rather than
            // being ranges. There is one per *declaration*, not per
            // element -- a document usually has a handful, all on the
            // root -- so the plumbing to span them would cost more
            // than it saves.
            let prefix = self.doc.push_expanded("xml");
            let uri = self
                .doc
                .push_expanded("http://www.w3.org/XML/1998/namespace");
            let id = self
                .doc
                .push_detached(NodeData::Namespace { prefix, uri }, node);
            self.doc.ns_ids.push(id);
        }
        for (prefix, uri) in declared {
            let prefix = self.doc.push_expanded(&prefix);
            let uri = self.doc.push_expanded(&uri);
            let id = self
                .doc
                .push_detached(NodeData::Namespace { prefix, uri }, node);
            self.doc.ns_ids.push(id);
        }
        (start, self.doc.ns_ids.len() - start)
    }

    fn parse_children(&mut self, node: NodeId, open_qname: &str) -> Result<()> {
        let mark = self.doc.scratch_mark();
        let mut text = Run::default();
        loop {
            // Checked once per child rather than at each `push`: every
            // node this parser creates is created from inside this loop
            // (directly, or by the recursive `parse_element` below), so
            // one check here bounds the whole arena without threading a
            // `Result` through the tree API.
            if self.limits.max_nodes.is_some_and(|m| self.doc.len() > m) {
                return Err(Error::new(ErrorKind::TooManyNodes, self.pos));
            }
            if self.pos >= self.bytes.len() {
                return Err(Error::new(ErrorKind::UnexpectedEof, self.pos));
            }
            if self.peek_is(b'<') {
                if self.starts_with("</") {
                    self.flush_text(&mut text, node)?;
                    let start = self.pos;
                    self.pos += 2;
                    let close = self.parse_name()?;
                    self.skip_whitespace();
                    if !self.peek_is(b'>') {
                        return Err(Error::new(
                            ErrorKind::Unterminated("end tag"),
                            start,
                        ));
                    }
                    self.pos += 1;
                    if close != open_qname {
                        return Err(Error::new(
                            ErrorKind::MismatchedEndTag {
                                expected: open_qname.to_owned(),
                                found: close.to_owned(),
                            },
                            start,
                        ));
                    }
                    // The element is closed, so its children are
                    // complete and can be moved into the flat arena as
                    // one contiguous block.
                    self.doc.finish_children(node, mark);
                    return Ok(());
                } else if self.starts_with("<!--") {
                    self.flush_text(&mut text, node)?;
                    let c = self.parse_comment()?;
                    let _ = self.doc.push(NodeData::Comment(c), node);
                } else if self.starts_with("<![CDATA[") {
                    self.parse_cdata(&mut text)?;
                } else if self.starts_with("<?") {
                    self.flush_text(&mut text, node)?;
                    let (t, d) = self.parse_pi()?;
                    let _ = self.doc.push(
                        NodeData::ProcessingInstruction { target: t, data: d },
                        node,
                    );
                } else {
                    self.flush_text(&mut text, node)?;
                    self.parse_element(node)?;
                }
            } else {
                self.parse_text_run(&mut text)?;
            }
        }
    }

    fn flush_text(&mut self, text: &mut Run, node: NodeId) -> Result<()> {
        if !text.is_empty() {
            let run = core::mem::take(text);
            if self.limits.max_text_length.is_some_and(|m| run.len() > m) {
                return Err(Error::new(ErrorKind::TextTooLong, self.pos));
            }
            // A run still expressible as a slice becomes a range; one
            // that is not joins the side table.
            let chars = if let Some((start, len)) = run.as_span() {
                self.verbatim(start, len)
            } else {
                let text = run.as_str(self.input);
                self.doc.push_expanded(text)
            };
            let _ = self.doc.push(NodeData::Text(chars), node);
        }
        Ok(())
    }

    pub(crate) fn parse_text_run(&mut self, out: &mut Run) -> Result<()> {
        while self.pos < self.bytes.len() {
            // `CharData ::= [^<&]* - ([^<&]* ']]>' [^<&]*)`. The
            // sequence is forbidden literally so that a reader can
            // always tell where a CDATA section ends; it must be
            // written `]]&gt;`.
            if self.starts_with("]]>") {
                return Err(Error::new(ErrorKind::IllegalCdataEnd, self.pos));
            }
            match self.bytes[self.pos] {
                b'<' => break,
                b'&' => {
                    let s = self.pos;
                    self.pos += 1;
                    let end =
                        self.input[self.pos..].find(';').ok_or_else(|| {
                            Error::new(ErrorKind::Unterminated("entity"), s)
                        })?;
                    let ent = &self.input[self.pos..self.pos + end];
                    // Element content, not an attribute value: section
                    // 3.3.3 does not apply, and a newline in content is
                    // content.
                    let expanded = self.expand_entity(ent, s, false)?;
                    // The one thing in a text run the input does not
                    // hold verbatim: this is where a run stops being a
                    // slice and starts being a string.
                    out.push_owned(&expanded, self.input);
                    self.pos += end + 1;
                }
                _ => {
                    let start = self.pos;
                    while self.pos < self.bytes.len()
                        && self.bytes[self.pos] != b'<'
                        && self.bytes[self.pos] != b'&'
                    {
                        self.pos += 1;
                    }
                    // The check at the top of the loop only sees the
                    // first byte of a run. Everything after it was
                    // consumed here without being looked at, so
                    // `]]]>`, `abc]]>def` and the text left over after
                    // a nested CDATA section all passed.
                    let run = &self.input[start..self.pos];
                    if let Some(at) = run.find("]]>") {
                        return Err(Error::new(
                            ErrorKind::IllegalCdataEnd,
                            start + at,
                        ));
                    }
                    out.push_slice(start, run, self.input);
                }
            }
        }
        Ok(())
    }

    /// Consumes a `CDATA` section into a character-data run.
    ///
    /// CDATA is character data, so it joins the run rather than
    /// becoming a node of its own -- `a<![CDATA[b]]>c` is the single
    /// text `abc`. Extracted so the streaming reader consumes CDATA
    /// exactly as the tree parser does; when it had its own copy, the
    /// copy did not advance and read the section forever.
    pub(crate) fn parse_cdata(&mut self, out: &mut Run) -> Result<()> {
        self.pos += "<![CDATA[".len();
        let end = self.input[self.pos..].find("]]>").ok_or_else(|| {
            Error::new(ErrorKind::Unterminated("CDATA"), self.pos)
        })?;
        // The body is itself a slice of the input, so a text node that
        // is *only* a `CDATA` section still costs nothing. One mixed
        // with adjacent text does, because the delimiters sit between
        // them and the pieces are no longer contiguous.
        let body = &self.input[self.pos..self.pos + end];
        out.push_slice(self.pos, body, self.input);
        self.pos += end + 3;
        Ok(())
    }

    /// An entity's replacement text: character references included,
    /// general entity references left alone.
    ///
    /// XML 1.0 section 4.4: inside an `EntityValue`, character
    /// references are *included* -- expanded there and then -- while
    /// general entity references are *bypassed* and left for the point
    /// of use. That distinction is the whole of why
    /// `<!ENTITY e "&#38;">` is a trap and `<!ENTITY e "&amp;">` is
    /// not: the first has the single character `&` as its replacement
    /// text, which is markup wherever it lands and starts no
    /// reference, while the second still holds a reference that
    /// resolves cleanly when it is used.
    ///
    /// This parser stores entity values as written, so the inclusion
    /// is done here instead, for the benefit of the checks below.
    fn replacement_text(&self, text: &str, offset: usize) -> Result<String> {
        let mut out = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(amp) = rest.find('&') {
            out.push_str(&rest[..amp]);
            let tail = &rest[amp..];
            let Some(semi) = tail.find(';') else {
                // Not a reference at all. Left as written so the
                // content check reports it.
                out.push_str(tail);
                return Ok(out);
            };
            let name = &tail[1..semi];
            if let Some(rest_name) = name.strip_prefix('#') {
                let _ = rest_name;
                match decode_predefined(name) {
                    Some(direct) => {
                        if let Some(bad) = direct
                            .chars()
                            .find(|c| !is_xml_char_for(*c, self.version))
                        {
                            return Err(Error::new(
                                ErrorKind::IllegalCharacter(bad),
                                offset,
                            ));
                        }
                        out.push_str(&direct);
                    }
                    None => out.push_str(&tail[..=semi]),
                }
            } else {
                // Bypassed: a general entity reference stays a
                // reference until it is used.
                out.push_str(&tail[..=semi]);
            }
            rest = &tail[semi + 1..];
        }
        out.push_str(rest);
        Ok(out)
    }

    /// Check replacement text included in an attribute value.
    ///
    /// XML 1.0 section 4.4.5, *Included in Literal*: the replacement
    /// text is parsed as though it were the literal, so `<` is
    /// forbidden and `&` must begin a reference. `<!ENTITY e "&#38;">`
    /// used as `a="&e;"` therefore fails, while `a="&amp;"` -- a
    /// predefined reference, not an entity's replacement text --
    /// does not.
    fn check_entity_in_attribute(text: &str, offset: usize) -> Result<()> {
        let bytes = text.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'<' => {
                    return Err(Error::new(
                        ErrorKind::IllegalCharacter('<'),
                        offset,
                    ));
                }
                b'&' => {
                    // Must be a complete reference *within* the
                    // replacement text: a document may not assemble one
                    // out of an entity plus the characters after it.
                    let rest = &text[i + 1..];
                    let Some(semi) = rest.find(';') else {
                        return Err(Error::new(
                            ErrorKind::Unterminated("entity"),
                            offset,
                        ));
                    };
                    if semi == 0 {
                        return Err(Error::new(ErrorKind::InvalidName, offset));
                    }
                    i += semi + 2;
                }
                _ => i += 1,
            }
        }
        Ok(())
    }

    /// Check an entity's replacement text as though it were markup.
    ///
    /// XML 1.0 section 4.4.2: a general entity referenced from content
    /// is *included*, which means its replacement text is parsed as
    /// content -- not substituted as characters. So
    /// `<!ENTITY e "</foo><foo>">` closes an element it never opened,
    /// `<!ENTITY e "&#60;foo>">` opens one it never closes, and
    /// `<!ENTITY e "&#38;">` yields a bare `&` where a reference is
    /// required. All three are documents this parser used to accept.
    ///
    /// Character references and the five predefined entities do **not**
    /// go through here. `&#38;` in content is the character `&` and is
    /// text; it is only when that `&` arrives as the replacement text
    /// of a declared entity that it has to be markup.
    ///
    /// The replacement text is parsed with this same parser into a
    /// document that is then thrown away. Reusing the scanner rather
    /// than writing a second one is the point: attribute-value rules,
    /// reserved processing-instruction targets, name validity and
    /// comment termination are all enforced here without being
    /// restated, and cannot drift from what the tree parser does.
    ///
    /// # Cost
    ///
    /// This runs once per *reference*, not once per entity, so a
    /// document that references one entity a thousand times parses its
    /// replacement text a thousand times. That cost is real and is not
    /// quantified here: the machine available while this was written
    /// measured the same benchmark between 1.45 and 3.76 ms in one
    /// state, so no honest before-and-after could be taken. See
    /// `doc/BENCHMARKS.md` for why an absolute figure is not published.
    ///
    /// Memoising by entity name is the obvious fix and is **not**
    /// obviously sound: validity depends on the namespace bindings in
    /// scope, so `<!ENTITY e "<p:x/>">` is well-formed where `p` is
    /// bound and not where it is not. A cache would have to key on the
    /// scope as well as the name, and a wrong key here accepts
    /// documents that should be refused -- which is the bug this
    /// function exists to fix. Left undone deliberately.
    fn check_entity_as_content(
        &mut self,
        text: &str,
        offset: usize,
    ) -> Result<()> {
        if self.entity_depth >= self.limits.max_entity_depth {
            return Err(Error::new(ErrorKind::EntityLimitExceeded, offset));
        }
        let mut sub = Parser {
            input: text,
            bytes: text.as_bytes(),
            pos: 0,
            // Thrown away: only whether it parses matters, so the
            // ranges it records point nowhere and are never read.
            doc: Document::with_capacity(0),
            // Cloned so a prefix bound outside the entity resolves
            // inside it, and so bindings the entity makes do not
            // escape.
            ns: self.ns.clone(),
            depth: self.depth,
            limits: self.limits,
            dtd: self.dtd.clone(),
            version: self.version,
            name_index: alloc::collections::BTreeMap::new(),
            external: self.external,
            entity_budget: self.entity_budget,
            entity_depth: self.entity_depth + 1,
        };
        let result = sub.parse_entity_body();
        // The budget is per document, so what the check spent counts.
        self.entity_budget = sub.entity_budget;
        // An offset into the replacement text means nothing to a
        // caller holding the document, so errors are reported at the
        // reference that pulled the text in.
        result.map_err(|e| Error::new(e.kind, offset))
    }

    /// Parse replacement text as `content`, to the end of it.
    ///
    /// Differs from [`Parser::parse_children`] in where it stops: this
    /// ends at the end of the text rather than at an end tag, and an
    /// end tag with nothing open is an error rather than the signal to
    /// return.
    fn parse_entity_body(&mut self) -> Result<()> {
        let root = self.doc.root();
        let mark = self.doc.scratch_mark();
        while self.pos < self.bytes.len() {
            if !self.peek_is(b'<') {
                let mut run = Run::default();
                self.parse_text_run(&mut run)?;
                continue;
            }
            if self.starts_with("</") {
                // The entity closes an element it did not open, so the
                // replacement text is not `content` and including it
                // would unbalance the document around it.
                let name = self.peek_end_tag_name();
                return Err(Error::new(
                    ErrorKind::UnexpectedEndTag(name),
                    self.pos,
                ));
            }
            if self.starts_with("<!--") {
                let _ = self.parse_comment()?;
            } else if self.starts_with("<![CDATA[") {
                let mut run = Run::default();
                self.parse_cdata(&mut run)?;
            } else if self.starts_with("<?") {
                let _ = self.parse_pi()?;
            } else {
                // `parse_element` requires a matching end tag, so an
                // element opened here and left open is reported.
                self.parse_element(root)?;
            }
        }
        self.doc.finish_children(root, mark);
        Ok(())
    }

    /// Resolve stored character data against the text being scanned.
    ///
    /// The streaming reader needs the text itself rather than a range,
    /// because an event outlives the scan that produced it.
    pub(crate) fn owned(&self, c: Chars) -> &str {
        match c {
            Chars::Span(start, len) => {
                let (start, len) = (start as usize, len as usize);
                self.input.get(start..start + len).unwrap_or_default()
            }
            Chars::Expanded(i) => self
                .doc
                .expanded
                .get(i as usize)
                .map_or("", alloc::string::String::as_str),
        }
    }

    /// Record a verbatim slice of the input as a range into it.
    ///
    /// This is where an allocation used to be. The document owns the
    /// text already; a node that repeats it owns nothing.
    pub(crate) fn verbatim(&mut self, start: usize, len: usize) -> Chars {
        if let (Ok(s), Ok(l)) = (u32::try_from(start), u32::try_from(len)) {
            return Chars::Span(s, l);
        }
        // Past what a 32-bit range can address. Keep the text in the
        // side table rather than truncate the document.
        let text = &self.input[start..start + len];
        self.doc.push_expanded(text)
    }

    pub(crate) fn parse_comment(&mut self) -> Result<Chars> {
        let start = self.pos;
        self.pos += "<!--".len();
        let end = self.input[self.pos..].find("-->").ok_or_else(|| {
            Error::new(ErrorKind::Unterminated("comment"), start)
        })?;
        let body = &self.input[self.pos..self.pos + end];
        // `Comment ::= '<!--' ((Char - '-') | ('-' (Char - '-')))* '-->'`
        // — `--` may not appear inside, and the body may not end with a
        // single `-` (which would make the terminator `--->`).
        if body.contains("--") || body.ends_with('-') {
            return Err(Error::new(ErrorKind::MalformedComment, start));
        }
        let at = self.pos;
        let len = body.len();
        self.pos += end + 3;
        Ok(self.verbatim(at, len))
    }

    pub(crate) fn parse_pi(&mut self) -> Result<(Chars, Chars)> {
        let start = self.pos;
        self.pos += 2; // '<?'
        let target_at = self.pos;
        let target = self.parse_name()?;
        let target_len = target.len();
        // `PITarget ::= Name - (('X'|'x')('M'|'m')('L'|'l'))`. The name
        // `xml` in any case is reserved, so a second XML declaration
        // later in the document is not merely misplaced — it is not a
        // legal processing instruction at all.
        if target.eq_ignore_ascii_case("xml") {
            return Err(Error::new(ErrorKind::ReservedPiTarget, self.pos));
        }
        // Namespaces in XML narrows `PITarget` from `Name` to `NCName`,
        // which has no colon. A namespace-aware parser must reject
        // `<?a:b ?>`; `Name` alone would allow it, which is why this is
        // easy to miss.
        if target.contains(':') {
            return Err(Error::new(ErrorKind::InvalidName, self.pos));
        }
        // The target must be followed by whitespace or `?>`; anything
        // else means the name stopped early on an illegal character.
        if !matches!(
            self.bytes.get(self.pos),
            Some(b' ' | b'\t' | b'\r' | b'\n')
        ) && !self.starts_with("?>")
        {
            return Err(Error::new(ErrorKind::InvalidName, self.pos));
        }
        let end = self.input[self.pos..].find("?>").ok_or_else(|| {
            Error::new(ErrorKind::Unterminated("processing instruction"), start)
        })?;
        // Trimming a slice yields a slice of the same buffer, so the
        // trimmed data is still a range into the input rather than
        // anything that has to be built.
        let raw = &self.input[self.pos..self.pos + end];
        let lead = raw.len() - raw.trim_start().len();
        let data_at = self.pos + lead;
        let data_len = raw.trim().len();
        self.pos += end + 2;
        let target = self.verbatim(target_at, target_len);
        let data = self.verbatim(data_at, data_len);
        Ok((target, data))
    }

    fn parse_attributes(&mut self) -> Result<Vec<(&'a str, Run)>> {
        let mut out = Vec::new();
        loop {
            self.skip_whitespace();
            if self.pos >= self.bytes.len() {
                return Err(Error::new(ErrorKind::UnexpectedEof, self.pos));
            }
            let b = self.bytes[self.pos];
            if b == b'>' || b == b'/' {
                return Ok(out);
            }
            let name = self.parse_name()?;
            self.skip_whitespace();
            if !self.peek_is(b'=') {
                return Err(Error::new(
                    ErrorKind::UnquotedAttributeValue,
                    self.pos,
                ));
            }
            self.pos += 1;
            self.skip_whitespace();
            let value = self.parse_attribute_value()?;
            // `STag ::= '<' Name (S Attribute)* S? '>'` — attributes are
            // separated by whitespace, so `<a b="c"d="e"/>` is not
            // well-formed even though both attributes parse.
            // `None` falls through: running out of input is unexpected
            // EOF, reported by the loop above, not a missing separator.
            if !matches!(
                self.bytes.get(self.pos),
                None | Some(b' ' | b'\t' | b'\r' | b'\n' | b'>' | b'/')
            ) {
                return Err(Error::new(
                    ErrorKind::UnquotedAttributeValue,
                    self.pos,
                ));
            }
            if value.len() > self.limits.max_attribute_size {
                return Err(Error::new(ErrorKind::AttributeTooLarge, self.pos));
            }
            if out.len() >= self.limits.max_attributes_per_element {
                return Err(Error::new(ErrorKind::TooManyAttributes, self.pos));
            }
            out.push((name, value));
        }
    }

    /// `AttValue ::= '"' ([^<&"] | Reference)* '"' | "'" ([^<&'] | Reference)* "'"`
    ///
    /// A literal `<` is forbidden — it must be written `&lt;` — because
    /// otherwise an unclosed tag inside a value is indistinguishable
    /// from markup.
    fn parse_attribute_value(&mut self) -> Result<Run> {
        let start = self.pos;
        if self.pos >= self.bytes.len() {
            return Err(Error::new(ErrorKind::UnexpectedEof, start));
        }
        let quote = self.bytes[self.pos];
        if quote != b'"' && quote != b'\'' {
            return Err(Error::new(ErrorKind::UnquotedAttributeValue, start));
        }
        self.pos += 1;
        let mut out = Run::default();
        loop {
            if self.pos >= self.bytes.len() {
                return Err(Error::new(
                    ErrorKind::Unterminated("attribute value"),
                    start,
                ));
            }
            match self.bytes[self.pos] {
                b if b == quote => {
                    self.pos += 1;
                    return Ok(out);
                }
                b'<' => {
                    return Err(Error::new(
                        ErrorKind::IllegalCharacter('<'),
                        self.pos,
                    ));
                }
                b'&' => {
                    let s = self.pos;
                    self.pos += 1;
                    let end =
                        self.input[self.pos..].find(';').ok_or_else(|| {
                            Error::new(ErrorKind::Unterminated("entity"), s)
                        })?;
                    let ent = &self.input[self.pos..self.pos + end];
                    // Normalisation happens inside the expansion, so
                    // that a character reference within an entity's
                    // replacement text stays exempt.
                    let expanded = self.expand_entity(ent, s, true)?;
                    out.push_owned(&expanded, self.input);
                    self.pos += end + 1;
                }
                _ => {
                    // Stopping at `<` as well lets the arm above report
                    // it. Without this the run swallowed it, so
                    // `a="<x"` was rejected and `a="1 < 2"` was not.
                    let s = self.pos;
                    while self.pos < self.bytes.len()
                        && self.bytes[self.pos] != quote
                        && self.bytes[self.pos] != b'&'
                        && self.bytes[self.pos] != b'<'
                    {
                        self.pos += 1;
                    }
                    // Normalisation only rewrites a value that
                    // contains a tab or a line break, which almost
                    // none do. When it would change nothing, the value
                    // is a slice of the input and costs nothing.
                    let raw = &self.input[s..self.pos];
                    if raw.bytes().any(|b| matches!(b, b'\t' | b'\n' | b'\r')) {
                        let mut normalized = String::new();
                        push_attribute_normalized(&mut normalized, raw);
                        out.push_owned(&normalized, self.input);
                    } else {
                        out.push_slice(s, raw, self.input);
                    }
                }
            }
        }
    }

    pub(crate) fn parse_name(&mut self) -> Result<&'a str> {
        let name = self.parse_name_unchecked()?;
        if name.len() > self.limits.max_name_length {
            return Err(Error::new(ErrorKind::NameTooLong, self.pos));
        }
        Ok(name)
    }

    /// A name, borrowed from the input.
    ///
    /// Every name in a document is a slice of the input already, and
    /// copying each one out cost an allocation per element and per
    /// attribute -- roughly 12,000 on a 16,002-node document, all of
    /// them discarded immediately after the name was interned.
    ///
    /// The returned slice borrows the input rather than the parser, so
    /// it outlives the `&mut self` that produced it.
    fn parse_name_unchecked(&mut self) -> Result<&'a str> {
        // Copy the reference, so the result does not reborrow `self`.
        let input: &'a str = self.input;
        let start = self.pos;
        let rest = &input[self.pos..];
        let mut chars = rest.char_indices();
        match chars.next() {
            Some((_, c)) if self.is_name_start(c) => {}
            _ => {
                return Err(Error::new(ErrorKind::InvalidName, start));
            }
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

    /// Split a `QName` and resolve its prefix.
    ///
    /// `is_element` matters: an unprefixed *element* name takes the
    /// default namespace, an unprefixed *attribute* name is in no
    /// namespace at all. Conflating the two is the classic namespace
    /// bug, so the distinction is a parameter rather than an
    /// assumption.
    /// Resolve a qualified name to borrowed parts, without allocating.
    ///
    /// The namespace URI is borrowed from the in-scope declarations and
    /// the local part from the input, so a name that has been seen
    /// before costs nothing to look up.
    fn resolve_parts<'q>(
        &'q self,
        qname: &'q str,
        is_element: bool,
        offset: usize,
    ) -> Result<(&'q str, Option<&'q str>)> {
        match qname.split_once(':') {
            Some((prefix, local)) => {
                // `xml` is bound by specification and never declared.
                if prefix == "xml" {
                    return Ok((
                        local,
                        Some("http://www.w3.org/XML/1998/namespace"),
                    ));
                }
                let uri = self.ns.resolve(prefix).ok_or_else(|| {
                    Error::new(
                        ErrorKind::UnboundPrefix(prefix.to_owned()),
                        offset,
                    )
                })?;
                Ok((local, Some(uri)))
            }
            None if is_element => match self.ns.resolve("") {
                Some(uri) if !uri.is_empty() => Ok((qname, Some(uri))),
                _ => Ok((qname, None)),
            },
            // An unprefixed *attribute* is in no namespace even when a
            // default namespace is declared. That asymmetry with
            // elements comes from the specification.
            None => Ok((qname, None)),
        }
    }

    /// Resolve a qualified name and intern it, in one step.
    ///
    /// Going through `expand` first allocated an `ExpandedName` for
    /// every element and attribute *before* the lookup, so interning
    /// reduced what the document retained and not what it allocated --
    /// the figure did not move at all. Looking up by borrowed parts
    /// means a repeated name costs a map probe and nothing else, and
    /// only a genuinely new name allocates.
    pub(crate) fn intern_qname(
        &mut self,
        qname: &str,
        is_element: bool,
        offset: usize,
    ) -> Result<crate::tree::NameId> {
        let prefix = qname.split_once(':').map(|(p, _)| p);
        {
            let (local, namespace) =
                self.resolve_parts(qname, is_element, offset)?;
            if let Some(candidates) = self.name_index.get(local) {
                for &id in candidates {
                    // The prefix has to match as well as the namespace.
                    // Two prefixes bound to one namespace expand to the
                    // same name, so keying on the expansion alone gave
                    // them one id -- and then `name()` had to report a
                    // single prefix for both, which is wrong for one of
                    // them. Distinct prefixes get distinct ids; the
                    // cost is one extra entry per prefix actually used.
                    if self.doc.names[id as usize].namespace.as_deref()
                        == namespace
                        && self.doc.name_prefixes[id as usize].as_deref()
                            == prefix
                    {
                        return Ok(crate::tree::NameId(id));
                    }
                }
            }
        }
        // Cold path: a name this document has not used before.
        let name = self.expand(qname, is_element, offset)?;
        Ok(self.intern(&name, prefix))
    }

    fn expand(
        &self,
        qname: &str,
        is_element: bool,
        offset: usize,
    ) -> Result<ExpandedName> {
        match qname.split_once(':') {
            Some((prefix, local)) => {
                // `xml` is bound by specification and never declared.
                if prefix == "xml" {
                    return Ok(ExpandedName::qualified(
                        "http://www.w3.org/XML/1998/namespace",
                        local,
                    ));
                }
                let uri = self.ns.resolve(prefix).ok_or_else(|| {
                    Error::new(
                        ErrorKind::UnboundPrefix(prefix.to_owned()),
                        offset,
                    )
                })?;
                Ok(ExpandedName::qualified(uri, local))
            }
            None => {
                if is_element {
                    match self.ns.resolve("") {
                        Some(uri) if !uri.is_empty() => {
                            Ok(ExpandedName::qualified(uri, qname))
                        }
                        _ => Ok(ExpandedName::local(qname)),
                    }
                } else {
                    Ok(ExpandedName::local(qname))
                }
            }
        }
    }

    pub(crate) fn peek_end_tag_name(&self) -> String {
        self.input[self.pos + 2..]
            .split(['>', ' ', '\t', '\n', '\r'])
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    pub(crate) fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    pub(crate) fn peek_is(&self, b: u8) -> bool {
        self.bytes.get(self.pos) == Some(&b)
    }

    pub(crate) fn skip_whitespace(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\r' | b'\n')
        {
            self.pos += 1;
        }
    }

    /// `NameStartChar`, for the edition in force.
    fn is_name_start(&self, c: char) -> bool {
        match self.limits.edition {
            crate::Edition::Fourth => crate::names4e::is_name_start_4e(c),
            _ => is_name_start(c),
        }
    }

    /// `NameChar`, for the edition in force.
    fn is_name_char(&self, c: char) -> bool {
        match self.limits.edition {
            crate::Edition::Fourth => crate::names4e::is_name_char_4e(c),
            _ => is_name_char(c),
        }
    }

    /// Resolve one entity reference, consulting the DTD.
    ///
    /// The five predefined entities and numeric character references
    /// resolve directly. Anything else must have been declared, and
    /// only *internal* declarations carry replacement text — `oxml`
    /// never fetches an external resource, which is what makes XXE
    /// structurally impossible rather than merely mitigated.
    ///
    /// An undeclared entity is an error only when the declaration was
    /// complete enough to be sure. If it had an external subset or
    /// referenced a parameter entity, the declaration may exist
    /// somewhere we did not read, and rejecting would be wrong.
    /// Expand one reference.
    ///
    /// `in_attribute` carries XML section 3.3.3 through the recursion:
    /// inside an attribute value, literal whitespace in an entity's
    /// replacement text becomes a space, but a **character reference**
    /// anywhere in it is exempt. Normalising the finished expansion
    /// instead would lose that distinction, because by then `&#xA;`
    /// and a literal newline are the same character.
    fn expand_entity(
        &mut self,
        ent: &str,
        offset: usize,
        in_attribute: bool,
    ) -> Result<String> {
        if let Some(direct) = decode_predefined(ent) {
            // A character *reference* may not name a character the
            // `Char` production forbids. `&#0;` is illegal in both 1.0
            // and 1.1; the C0 controls are legal as references in 1.1
            // only. Decoding without this check accepted 65 documents
            // that are not well-formed.
            if let Some(bad) =
                direct.chars().find(|c| !is_xml_char_for(*c, self.version))
            {
                return Err(Error::new(
                    ErrorKind::IllegalCharacter(bad),
                    offset,
                ));
            }
            return Ok(direct);
        }
        let Some(dtd) = self.dtd.as_ref() else {
            return Err(Error::new(
                ErrorKind::UnknownEntity(ent.to_owned()),
                offset,
            ));
        };
        match dtd.entity(ent) {
            Some(crate::dtd::EntityValue::Internal(text)) => {
                let text = text.clone();
                // Included, not substituted: the replacement text of a
                // declared entity is markup where it lands.
                let replacement = self.replacement_text(&text, offset)?;
                if in_attribute {
                    Self::check_entity_in_attribute(&replacement, offset)?;
                } else {
                    self.check_entity_as_content(&replacement, offset)?;
                }
                let mut budget = self.entity_budget;
                let out = self.expand_text(
                    &text,
                    offset,
                    1,
                    &mut budget,
                    in_attribute,
                );
                self.entity_budget = budget;
                out
            }
            // `WFC: No External Entity References`. In content an
            // external parsed entity expands to nothing, because
            // nothing is ever fetched; in an attribute value the
            // reference itself is forbidden.
            Some(crate::dtd::EntityValue::External { system, public }) => {
                if in_attribute {
                    return Err(Error::new(
                        ErrorKind::ForbiddenEntityReference(ent.to_owned()),
                        offset,
                    ));
                }
                // Whatever the caller made available, minus its text
                // declaration, which is markup about the entity rather
                // than part of it. Without a source this is `None` and
                // the reference expands to nothing, as before.
                let (system, public) = (system.clone(), public.clone());
                match self.external.fetch(&system, public.as_deref()) {
                    Some(content) => {
                        // Each external parsed entity has its own
                        // version and is normalised independently. An
                        // entity declaring 1.1 may use U+2028 as
                        // whitespace -- including inside its own text
                        // declaration, which is where this first showed
                        // up: the declaration was reported malformed
                        // because the separator had not been
                        // normalised yet.
                        let entity_version =
                            entity_version(content, self.version);
                        let normalized =
                            normalize_line_endings(content, entity_version);
                        let content: &str = &normalized;
                        check_text_decl_position(content, offset)?;
                        check_text_decl(content, self.version, offset)?;
                        let body = strip_text_decl(content);
                        // The content is parsed for illegal characters
                        // the same way the document was; it arrived
                        // from outside and is no more trusted.
                        if let Some((at, c)) =
                            body.char_indices().find(|(_, c)| {
                                !is_literal_char_for(*c, self.version)
                            })
                        {
                            let _ = at;
                            return Err(Error::new(
                                ErrorKind::IllegalCharacter(c),
                                offset,
                            ));
                        }
                        // An external parsed entity is *included*
                        // exactly as an internal one is, so its
                        // replacement text is content and not
                        // characters. Checking only internal entities
                        // left `<root&#x85;/>` -- NEL, whitespace in
                        // 1.1 and not in 1.0 -- accepted inside a 1.0
                        // document, because nothing ever read the
                        // replacement text as a tag.
                        if !in_attribute {
                            self.check_entity_as_content(body, offset)?;
                        }
                        let mut budget = self.entity_budget;
                        let out = self.expand_text(
                            body,
                            offset,
                            1,
                            &mut budget,
                            false,
                        );
                        self.entity_budget = budget;
                        out
                    }
                    None => Ok(String::new()),
                }
            }
            // `WFC: Parsed Entity`. An unparsed entity may not be
            // referenced anywhere -- it is not text and has no
            // replacement. Naming it as the value of an `ENTITY`-typed
            // attribute is a different construct and still allowed.
            Some(crate::dtd::EntityValue::Unparsed) => Err(Error::new(
                ErrorKind::ForbiddenEntityReference(ent.to_owned()),
                offset,
            )),
            None if dtd.incomplete => Ok(String::new()),
            None => Err(Error::new(
                ErrorKind::UnknownEntity(ent.to_owned()),
                offset,
            )),
        }
    }

    /// Expand entity references inside replacement text.
    ///
    /// Bounded on both axes, because either alone is insufficient:
    /// depth stops the exponential billion-laughs shape, and the
    /// character budget stops the quadratic variant where one large
    /// entity is referenced many times at depth one.
    /// Append a run of literal text, normalising it when it is part of
    /// an attribute value.
    /// Intern an element name, returning a handle to it.
    ///
    /// Names repeat heavily -- a catalogue of 2,000 items has a handful
    /// of distinct element names -- so storing an `ExpandedName` per
    /// element allocated thousands of strings to hold a few values.
    fn intern(
        &mut self,
        name: &ExpandedName,
        prefix: Option<&str>,
    ) -> crate::tree::NameId {
        if let Some(candidates) = self.name_index.get(name.local.as_str()) {
            for &id in candidates {
                if self.doc.names[id as usize].namespace == name.namespace
                    && self.doc.name_prefixes[id as usize].as_deref() == prefix
                {
                    return crate::tree::NameId(id);
                }
            }
        }
        let id = u32::try_from(self.doc.names.len()).unwrap_or(u32::MAX);
        self.doc.names.push(name.clone());
        self.doc.name_prefixes.push(prefix.map(str::to_owned));
        self.name_index
            .entry(name.local.clone())
            .or_default()
            .push(id);
        crate::tree::NameId(id)
    }

    fn push_run(
        out: &mut String,
        text: &str,
        offset: usize,
        budget: &mut usize,
        in_attribute: bool,
    ) -> Result<()> {
        // `WFC: No < in Attribute Values`. A literal `<` is legal in an
        // entity's *value*, and illegal in an attribute value -- so an
        // entity whose replacement text contains one may not be
        // referenced from an attribute, however indirectly. Checking
        // here rather than on the finished expansion catches it at any
        // depth, and leaves `&#60;` alone: a character reference stands
        // for the character and is permitted.
        if in_attribute && text.contains('<') {
            return Err(Error::new(ErrorKind::IllegalCharacter('<'), offset));
        }
        if in_attribute && text.contains(['\n', '\t', '\r']) {
            let mut buf = String::with_capacity(text.len());
            push_attribute_normalized(&mut buf, text);
            return Self::push_bounded(out, &buf, offset, budget);
        }
        Self::push_bounded(out, text, offset, budget)
    }

    fn expand_text(
        &mut self,
        text: &str,
        offset: usize,
        depth: usize,
        budget: &mut usize,
        in_attribute: bool,
    ) -> Result<String> {
        if depth > self.limits.max_entity_depth {
            return Err(Error::new(ErrorKind::EntityLimitExceeded, offset));
        }
        let mut out = String::new();
        let mut rest = text;
        // A `&` inside a CDATA section is text, not a reference.
        // Scanning for `&` without knowing that reported
        // `<![CDATA[&foo;]]>` as a reference to an undeclared entity --
        // which is how enforcing `Entity Declared` here first broke a
        // document that was correct.
        while let Some(amp) = next_reference(rest) {
            let (before, tail) = rest.split_at(amp);
            Self::push_run(&mut out, before, offset, budget, in_attribute)?;
            let Some(semi) = tail.find(';') else {
                return Err(Error::new(
                    ErrorKind::UnknownEntity(tail.to_owned()),
                    offset,
                ));
            };
            let name = &tail[1..semi];
            rest = &tail[semi + 1..];
            if let Some(direct) = decode_predefined(name) {
                // A character reference is exempt from attribute-value
                // normalisation; the five named entities expand to
                // characters that normalisation would not touch, so
                // both take the literal path.
                Self::push_bounded(&mut out, &direct, offset, budget)?;
                continue;
            }
            let inner = match self.dtd.as_ref().and_then(|d| d.entity(name)) {
                Some(crate::dtd::EntityValue::Internal(t)) => t.clone(),
                // An indirect reference is still a reference: an
                // internal entity whose text names an external one is
                // forbidden in an attribute value just as a direct
                // reference is.
                Some(crate::dtd::EntityValue::External { .. })
                    if in_attribute =>
                {
                    return Err(Error::new(
                        ErrorKind::ForbiddenEntityReference(name.to_owned()),
                        offset,
                    ));
                }
                Some(crate::dtd::EntityValue::Unparsed) => {
                    return Err(Error::new(
                        ErrorKind::ForbiddenEntityReference(name.to_owned()),
                        offset,
                    ));
                }
                // An entity named inside another entity's replacement
                // text must itself be declared. Skipping the reference
                // silently produced a document missing the content it
                // asked for, with nothing to say so.
                None if !self.dtd.as_ref().is_some_and(|d| d.incomplete) => {
                    return Err(Error::new(
                        ErrorKind::UnknownEntity(name.to_owned()),
                        offset,
                    ));
                }
                _ => continue,
            };
            // Already normalised by the recursive call if needed.
            let expanded = self.expand_text(
                &inner,
                offset,
                depth + 1,
                budget,
                in_attribute,
            )?;
            Self::push_bounded(&mut out, &expanded, offset, budget)?;
        }
        Self::push_run(&mut out, rest, offset, budget, in_attribute)?;
        Ok(out)
    }

    fn push_bounded(
        out: &mut String,
        text: &str,
        offset: usize,
        budget: &mut usize,
    ) -> Result<()> {
        if *budget < text.len() {
            return Err(Error::new(ErrorKind::EntityLimitExceeded, offset));
        }
        *budget -= text.len();
        out.push_str(text);
        Ok(())
    }
}

/// Resolve one entity reference.
///
/// Returns `None` when `ent` is a declared-entity name rather than one
/// of the five predefined entities or a numeric character reference —
/// resolving those needs the DTD, and is done by `expand_entity`.
fn decode_predefined(ent: &str) -> Option<String> {
    let out = match ent {
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "amp" => "&".to_owned(),
        "apos" => "'".to_owned(),
        "quot" => "\"".to_owned(),
        _ => {
            // Not a character reference at all means it is a declared
            // name, and resolving that needs the DTD.
            let cp = match ent.strip_prefix("#x") {
                Some(hex) => u32::from_str_radix(hex, 16).ok()?,
                None => ent.strip_prefix('#')?.parse::<u32>().ok()?,
            };
            let ch = char::from_u32(cp)?;
            let mut s = String::new();
            s.push(ch);
            s
        }
    };
    Some(out)
}

/// The XML version a document declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Version {
    /// XML 1.0, the default when no declaration says otherwise.
    V10,
    /// XML 1.1.
    V11,
}

/// Check the structure of an XML declaration.
///
/// ```text
/// XMLDecl ::= '<?xml' VersionInfo EncodingDecl? SDDecl? S? '?>'
/// ```
///
/// The pseudo-attributes are **ordered** and each must be preceded by
/// whitespace. Scanning to `?>` without checking accepts
/// `version="1.0"standalone="yes"`, which has no separating space, and
/// accepts them in the wrong order.
/// Whether `value` matches `VersionNum ::= '1.' [0-9]+`.
///
/// Which version the parser then *implements* is a separate question,
/// answered by [`declared_version`]: `1.2` is a well-formed version
/// number naming a version this crate does not support, and the two
/// failures are different.
fn is_legal_version(value: &str) -> bool {
    value.strip_prefix("1.").is_some_and(|digits| {
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    })
}

/// An external entity's content without its text declaration.
///
/// The declaration describes the entity; it is not part of it, and
/// leaving it in would put `<?xml …?>` into the document as text.
/// The version an external entity declares, or the document's.
///
/// An entity with no text declaration inherits the document's version;
/// one that declares its own uses that. The distinction matters before
/// normalisation, because which characters are line terminators
/// depends on it.
pub(crate) fn entity_version(content: &str, document: Version) -> Version {
    let Some(rest) = content.strip_prefix("<?xml") else {
        return document;
    };
    if !rest.starts_with([' ', '\t', '\r', '\n']) {
        return document;
    }
    match declared_version(content) {
        // No `version` pseudo-attribute: `declared_version` answers
        // 1.0 by default, but an entity without one inherits.
        Ok(Version::V10) if !rest.contains("version") => document,
        Ok(version) => version,
        Err(_) => document,
    }
}

/// Reject a text declaration that is not where one may be.
///
/// `extParsedEnt ::= TextDecl? content` -- the declaration comes first
/// or not at all. A blank line before it, or content before it, makes
/// it an ordinary processing instruction with the reserved target
/// `xml`, which is not legal anywhere.
///
/// The target is reserved case-insensitively, so `<?XML …?>` is an
/// error even at the start: `TextDecl` spells it in lower case.
pub(crate) fn check_text_decl_position(
    content: &str,
    offset: usize,
) -> Result<()> {
    let mut at = 0;
    while let Some(i) = content[at..].find("<?") {
        let start = at + i;
        let after = &content[start + 2..];
        let end = after
            .find(|c: char| c.is_whitespace() || c == '?')
            .unwrap_or(after.len());
        let target = &after[..end];
        if target.eq_ignore_ascii_case("xml") && (start != 0 || target != "xml")
        {
            return Err(Error::new(ErrorKind::ReservedPiTarget, offset));
        }
        at = start + 2;
    }
    Ok(())
}

pub(crate) fn strip_text_decl(content: &str) -> &str {
    let Some(rest) = content.strip_prefix("<?xml") else {
        return content;
    };
    if !rest.starts_with([' ', '\t', '\r', '\n']) {
        return content;
    }
    match rest.find("?>") {
        Some(end) => &rest[end + 2..],
        None => content,
    }
}

/// Check the text declaration at the head of an external entity.
///
/// `TextDecl ::= '<?xml' VersionInfo? EncodingDecl S? '?>'`, and it
/// differs from a document's XML declaration in three ways that the
/// conformance suite tests heavily:
///
/// * `encoding` is **mandatory** here and optional there;
/// * `standalone` is **forbidden** here -- only a document may say it;
/// * the version, if given, may not be later than the document's. A
///   1.1 document may include a 1.0 entity; a 1.0 document may not
///   include a 1.1 one.
pub(crate) fn check_text_decl(
    content: &str,
    document_version: Version,
    offset: usize,
) -> Result<()> {
    let Some(rest) = content.strip_prefix("<?xml") else {
        // A text declaration is optional; its absence says nothing.
        return Ok(());
    };
    // `<?xmlfoo?>` is a processing instruction, not a declaration --
    // and a reserved target, which the parser proper reports.
    if !rest.starts_with([' ', '\t', '\r', '\n']) {
        return Ok(());
    }
    let Some(end) = rest.find("?>") else {
        return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
    };
    let decl = &rest[..end];

    // The same grammar as a document's declaration, so the same
    // checker: order, duplicates, quoting, legal values.
    validate_xml_declaration(decl, offset, false)?;

    let field = |name: &str| -> Option<&str> {
        let at = decl.find(name)?;
        let after = decl[at + name.len()..].trim_start();
        let after = after.strip_prefix('=')?.trim_start();
        let quote = after.chars().next()?;
        let body = after.get(1..)?;
        let close = body.find(quote)?;
        body.get(..close)
    };

    if field("encoding").is_none() {
        return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
    }
    if field("standalone").is_some() {
        return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
    }
    if let Some(version) = field("version") {
        let entity_is_11 = version == "1.1";
        if entity_is_11 && document_version == Version::V10 {
            return Err(Error::new(ErrorKind::UnsupportedVersion, offset));
        }
    }
    Ok(())
}

fn validate_xml_declaration(
    decl: &str,
    offset: usize,
    require_version: bool,
) -> Result<()> {
    let mut rest = decl;
    let mut seen: Vec<&str> = Vec::new();

    loop {
        let trimmed = rest.trim_start();
        if trimmed.is_empty() {
            break;
        }
        // Each pseudo-attribute must be separated from what precedes it
        // by whitespace.
        if trimmed.len() == rest.len() {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        }
        rest = trimmed;

        let name_end = rest
            .find(|c: char| !c.is_ascii_alphabetic())
            .unwrap_or(rest.len());
        let name = &rest[..name_end];
        if !matches!(name, "version" | "encoding" | "standalone") {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        }
        if seen.contains(&name) {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        }
        // Order is fixed by the grammar, not merely conventional.
        let rank = |n: &str| match n {
            "version" => 0u8,
            "encoding" => 1,
            _ => 2,
        };
        if seen.last().is_some_and(|prev| rank(name) <= rank(prev)) {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        }
        seen.push(name);

        let after = rest[name_end..].trim_start();
        let Some(after) = after.strip_prefix('=') else {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        };
        let after = after.trim_start();
        let Some(quote) =
            after.chars().next().filter(|c| *c == '"' || *c == '\'')
        else {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        };
        let body = &after[1..];
        let Some(close) = body.find(quote) else {
            return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
        };
        let value = &body[..close];
        match name {
            // `VersionNum ::= '1.' [0-9]+`. There was no arm for this
            // at all, so `version="1.0 "`, `"1.0?"` and `"1.0^"` were
            // accepted -- the declaration's own version string was the
            // one field nothing checked.
            "version" if !is_legal_version(value) => {
                return Err(Error::new(
                    ErrorKind::MalformedDeclaration,
                    offset,
                ));
            }
            "standalone" if !matches!(value, "yes" | "no") => {
                return Err(Error::new(
                    ErrorKind::MalformedDeclaration,
                    offset,
                ));
            }
            "encoding" if !crate::encoding::is_legal_encoding_name(value) => {
                return Err(Error::new(
                    ErrorKind::MalformedDeclaration,
                    offset,
                ));
            }
            _ => {}
        }
        rest = &body[close + 1..];
    }

    // `VersionInfo` is required in a document's XML declaration and
    // **optional** in an external entity's text declaration --
    // `TextDecl ::= '<?xml' VersionInfo? EncodingDecl S? '?>'`. Reusing
    // this checker without the distinction rejected
    // `<?xml encoding="UTF-8"?>`, which is a perfectly ordinary way to
    // begin an entity.
    if require_version && seen.first() != Some(&"version") {
        return Err(Error::new(ErrorKind::MalformedDeclaration, offset));
    }
    Ok(())
}

/// The XML version the declaration names, defaulting to 1.0.
///
/// # Errors
///
/// Returns [`Error`] if the version is not one this parser implements.
fn declared_version(input: &str) -> Result<Version> {
    let Some(rest) = input.strip_prefix("<?xml") else {
        return Ok(Version::V10);
    };
    let Some(end) = rest.find("?>") else {
        return Ok(Version::V10);
    };
    let decl = &rest[..end];
    let Some(at) = decl.find("version") else {
        return Ok(Version::V10);
    };
    let after = decl[at + "version".len()..].trim_start();
    let Some(after) = after.strip_prefix('=') else {
        return Ok(Version::V10);
    };
    let after = after.trim_start();
    let Some(quote) = after.chars().next() else {
        return Ok(Version::V10);
    };
    if quote != '"' && quote != '\'' {
        return Ok(Version::V10);
    }
    let body = &after[1..];
    let Some(close) = body.find(quote) else {
        return Ok(Version::V10);
    };
    match &body[..close] {
        "1.0" => Ok(Version::V10),
        "1.1" => Ok(Version::V11),
        // XML 1.0 5th edition made an unrecognised 1.x version a
        // *forwards-compatibility* matter rather than an error: a 1.0
        // processor should accept the document and process it as 1.0.
        v if v.starts_with("1.") => Ok(Version::V10),
        _ => Err(Error::new(ErrorKind::UnsupportedVersion, 0)),
    }
}

/// `Char`, for the version in force — the rule for a character
/// arriving via a **reference**.
///
/// XML 1.1 widens the production to admit the C0 and C1 controls.
#[must_use]
const fn is_xml_char_for(c: char, version: Version) -> bool {
    match version {
        Version::V10 => is_xml_char(c),
        Version::V11 => matches!(c,
            '\u{1}'..='\u{D7FF}'
            | '\u{E000}'..='\u{FFFD}'
            | '\u{10000}'..='\u{10FFFF}'
        ),
    }
}

/// `RestrictedChar`, XML 1.1 production 2a.
///
/// ```text
/// RestrictedChar ::= [#x1-#x8] | [#xB-#xC] | [#xE-#x1F]
///                  | [#x7F-#x84] | [#x86-#x9F]
/// ```
#[must_use]
const fn is_restricted_char(c: char) -> bool {
    matches!(c,
        '\u{1}'..='\u{8}' | '\u{B}'..='\u{C}' | '\u{E}'..='\u{1F}'
        | '\u{7F}'..='\u{84}' | '\u{86}'..='\u{9F}'
    )
}

/// Whether `c` may appear **literally** in the document text.
///
/// This is deliberately stricter than [`is_xml_char_for`]. XML 1.1
/// admits the C0 and C1 controls into `Char`, but production 2a
/// forbids them appearing literally: they must be written as character
/// references. Conflating the two rules accepts 63 documents the suite
/// marks not-well-formed — a control character pasted into a comment or
/// into content is exactly the case this separates.
#[must_use]
const fn is_literal_char_for(c: char, version: Version) -> bool {
    match version {
        Version::V10 => is_xml_char(c),
        Version::V11 => is_xml_char_for(c, version) && !is_restricted_char(c),
    }
}

/// Whether `c` matches XML 1.0 production 2, `Char`.
///
/// ```text
/// Char ::= #x9 | #xA | #xD | [#x20-#xD7FF] | [#xE000-#xFFFD]
///        | [#x10000-#x10FFFF]
/// ```
///
/// Most C0 control characters are **not** legal anywhere in an XML
/// document — not in content, not in an attribute value, not even
/// inside a comment. A parser that accepts them is accepting documents
/// that are not well-formed, and this accounted for 77 failures against
/// the W3C suite.
///
/// Surrogates cannot appear: Rust's `char` cannot hold one. `#xFFFE`
/// and `#xFFFF` can, and are excluded.
#[must_use]
pub(crate) const fn is_xml_char(c: char) -> bool {
    matches!(c,
        '\u{9}' | '\u{A}' | '\u{D}'
        | '\u{20}'..='\u{D7FF}'
        | '\u{E000}'..='\u{FFFD}'
        | '\u{10000}'..='\u{10FFFF}'
    )
}

pub(crate) fn is_name_start(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '_' | ':')
        || matches!(c as u32,
            0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF
            | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D
            | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF
            | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

pub(crate) fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9')
        || c as u32 == 0xB7
        || matches!(c as u32, 0x300..=0x36F | 0x203F..=0x2040)
}
