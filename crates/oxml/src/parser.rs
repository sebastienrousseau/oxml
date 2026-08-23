// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The parser: XML text in, [`Document`] out.
//!
//! A single forward pass over the input, byte-oriented on the hot
//! paths and `char`-aware only where XML actually requires it (name
//! characters). No backtracking, no intermediate token vector — the
//! tree is built directly as the scan proceeds.

use alloc::borrow::ToOwned;
use alloc::string::String;
use alloc::vec::Vec;

use crate::error::{Error, ErrorKind, Result};
use crate::limits::Limits;
use crate::tree::{Attribute, Document, ExpandedName, NodeId, NodeKind};

/// Namespace bindings in scope, as a stack of (prefix, uri) frames.
///
/// A `Vec` searched backwards rather than a map: scopes are tiny in
/// practice (a handful of bindings), and this keeps push/pop free of
/// allocation and hashing on the hot path.
#[derive(Debug, Default)]
struct Namespaces {
    bindings: Vec<(String, String)>,
    marks: Vec<usize>,
}

impl Namespaces {
    fn push_scope(&mut self) {
        self.marks.push(self.bindings.len());
    }

    fn pop_scope(&mut self) {
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

struct Parser<'a> {
    input: &'a str,
    bytes: &'a [u8],
    pos: usize,
    doc: Document,
    ns: Namespaces,
    /// How many elements are currently open. Bounded by
    /// [`Limits::max_depth`] so the recursion cannot exhaust the stack.
    depth: usize,
    /// Resource bounds for this parse.
    limits: Limits,
    /// Declarations from the document type declaration, once seen.
    dtd: Option<crate::dtd::Dtd>,
    /// The version the XML declaration names.
    ///
    /// 1.1 differs from 1.0 in three ways that matter here: NEL and
    /// LINE SEPARATOR normalise to LF, C1 controls must be escaped
    /// rather than appearing literally, and the `Char` production
    /// admits C0 controls when written as character references.
    version: Version,
    /// Characters of entity expansion still permitted **for the whole
    /// document**.
    ///
    /// Per-document rather than per-reference. A per-reference budget
    /// bounds the exponential billion-laughs shape but not the
    /// quadratic one: referencing a single 100 KB entity a thousand
    /// times at depth one produced 100 MB from 100 KB of input while
    /// every individual expansion stayed within its allowance.
    entity_budget: usize,
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
    // Checked once over the whole input rather than at each construct.
    // The `Char` production applies everywhere — content, attribute
    // values, comments, even the DTD — so a per-construct check would
    // have to be repeated in a dozen places and would be forgotten in
    // the thirteenth. One pass is also faster than one branch per
    // character in the hot loops.
    let version = declared_version(input)?;
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
        doc: Document::new(),
        ns: Namespaces::default(),
        depth: 0,
        limits,
        dtd: None,
        version,
        entity_budget: limits.max_entity_expansion,
    };
    p.parse_document()?;
    Ok(p.doc)
}

impl Parser<'_> {
    fn parse_document(&mut self) -> Result<()> {
        let root = self.doc.root();
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
                    let _ = self.doc.push(NodeKind::Comment(c), root);
                } else if self.starts_with("<?") {
                    let (t, d) = self.parse_pi()?;
                    let _ = self.doc.push(
                        NodeKind::ProcessingInstruction { target: t, data: d },
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
    fn skip_prolog(&mut self) -> Result<()> {
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
            validate_xml_declaration(decl, self.pos)?;
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
    fn skip_doctype(&mut self) -> Result<()> {
        // Parsed rather than skipped. Well-formedness constraints live
        // inside the DTD — a malformed `<!ATTLIST>` makes a document not
        // well-formed for *every* parser, validating or not — and the
        // general entity declarations are needed so that a document
        // using `&chapter1;` is not rejected as undeclared.
        let mut p = crate::dtd::DtdParser::new(
            self.input,
            self.pos,
            self.limits.edition,
        );
        match p.parse_doctype() {
            Ok(dtd) => {
                self.pos = p.pos;
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

    fn parse_element_inner(&mut self, parent: NodeId) -> Result<()> {
        let tag_start = self.pos;
        self.pos += 1; // '<'
        let qname = self.parse_name()?;

        // Attributes are collected before the element node is created:
        // namespace declarations among them must be in scope for the
        // element's *own* name to resolve.
        self.ns.push_scope();
        let raw_attrs = self.parse_attributes()?;
        for (name, value) in &raw_attrs {
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
                self.ns.bind(prefix.to_owned(), value.clone());
            } else if name == "xmlns" {
                self.ns.bind(String::new(), value.clone());
            }
        }

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

        let name = self.expand(&qname, true, tag_start)?;

        // Resolve attribute names first so duplicates are detected
        // before any node is created — otherwise a rejected element
        // would already be in the arena.
        let mut resolved: Vec<Attribute> = Vec::with_capacity(raw_attrs.len());
        for (raw, value) in raw_attrs {
            if raw == "xmlns" || raw.starts_with("xmlns:") {
                continue;
            }
            let an = self.expand(&raw, false, tag_start)?;
            if resolved.iter().any(|a| a.name == an) {
                self.ns.pop_scope();
                return Err(Error::new(
                    ErrorKind::DuplicateAttribute(raw),
                    tag_start,
                ));
            }
            resolved.push(Attribute { name: an, value });
        }

        let node = self.doc.push(
            NodeKind::Element {
                name: name.clone(),
                attributes: Vec::new(),
            },
            parent,
        );

        // Attribute nodes are parented to the element but kept out of
        // its `children`, so `child::` cannot see them.
        let mut attr_ids = Vec::with_capacity(resolved.len());
        for at in resolved {
            attr_ids.push(self.doc.push_detached(NodeKind::Attr(at), node));
        }
        if let Some(NodeKind::Element { attributes, .. }) =
            self.doc.kind_mut(node)
        {
            *attributes = attr_ids;
        }

        if self_closing {
            self.ns.pop_scope();
            return Ok(());
        }

        self.parse_children(node, &qname)?;
        self.ns.pop_scope();
        Ok(())
    }

    fn parse_children(&mut self, node: NodeId, open_qname: &str) -> Result<()> {
        let mut text = String::new();
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
                                found: close,
                            },
                            start,
                        ));
                    }
                    return Ok(());
                } else if self.starts_with("<!--") {
                    self.flush_text(&mut text, node)?;
                    let c = self.parse_comment()?;
                    let _ = self.doc.push(NodeKind::Comment(c), node);
                } else if self.starts_with("<![CDATA[") {
                    // CDATA is character data, so it joins the run
                    // rather than becoming its own node.
                    self.pos += "<![CDATA[".len();
                    let end = self.input[self.pos..].find("]]>").ok_or_else(
                        || {
                            Error::new(
                                ErrorKind::Unterminated("CDATA"),
                                self.pos,
                            )
                        },
                    )?;
                    text.push_str(&self.input[self.pos..self.pos + end]);
                    self.pos += end + 3;
                } else if self.starts_with("<?") {
                    self.flush_text(&mut text, node)?;
                    let (t, d) = self.parse_pi()?;
                    let _ = self.doc.push(
                        NodeKind::ProcessingInstruction { target: t, data: d },
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

    fn flush_text(&mut self, text: &mut String, node: NodeId) -> Result<()> {
        if !text.is_empty() {
            let owned = core::mem::take(text);
            if self.limits.max_text_length.is_some_and(|m| owned.len() > m) {
                return Err(Error::new(ErrorKind::TextTooLong, self.pos));
            }
            let _ = self.doc.push(NodeKind::Text(owned), node);
        }
        Ok(())
    }

    fn parse_text_run(&mut self, out: &mut String) -> Result<()> {
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
                    out.push_str(&self.expand_entity(ent, s)?);
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
                    out.push_str(&self.input[start..self.pos]);
                }
            }
        }
        Ok(())
    }

    fn parse_comment(&mut self) -> Result<String> {
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
        let body = body.to_owned();
        self.pos += end + 3;
        Ok(body)
    }

    fn parse_pi(&mut self) -> Result<(String, String)> {
        let start = self.pos;
        self.pos += 2; // '<?'
        let target = self.parse_name()?;
        // `PITarget ::= Name - (('X'|'x')('M'|'m')('L'|'l'))`. The name
        // `xml` in any case is reserved, so a second XML declaration
        // later in the document is not merely misplaced — it is not a
        // legal processing instruction at all.
        if target.eq_ignore_ascii_case("xml") {
            return Err(Error::new(ErrorKind::ReservedPiTarget, self.pos));
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
        let data = self.input[self.pos..self.pos + end].trim().to_owned();
        self.pos += end + 2;
        Ok((target, data))
    }

    fn parse_attributes(&mut self) -> Result<Vec<(String, String)>> {
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
    fn parse_attribute_value(&mut self) -> Result<String> {
        let start = self.pos;
        if self.pos >= self.bytes.len() {
            return Err(Error::new(ErrorKind::UnexpectedEof, start));
        }
        let quote = self.bytes[self.pos];
        if quote != b'"' && quote != b'\'' {
            return Err(Error::new(ErrorKind::UnquotedAttributeValue, start));
        }
        self.pos += 1;
        let mut out = String::new();
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
                    out.push_str(&self.expand_entity(ent, s)?);
                    self.pos += end + 1;
                }
                _ => {
                    let s = self.pos;
                    while self.pos < self.bytes.len()
                        && self.bytes[self.pos] != quote
                        && self.bytes[self.pos] != b'&'
                    {
                        self.pos += 1;
                    }
                    out.push_str(&self.input[s..self.pos]);
                }
            }
        }
    }

    fn parse_name(&mut self) -> Result<String> {
        let name = self.parse_name_unchecked()?;
        if name.len() > self.limits.max_name_length {
            return Err(Error::new(ErrorKind::NameTooLong, self.pos));
        }
        Ok(name)
    }

    fn parse_name_unchecked(&mut self) -> Result<String> {
        let start = self.pos;
        let rest = &self.input[self.pos..];
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
        Ok(rest[..end].to_owned())
    }

    /// Split a `QName` and resolve its prefix.
    ///
    /// `is_element` matters: an unprefixed *element* name takes the
    /// default namespace, an unprefixed *attribute* name is in no
    /// namespace at all. Conflating the two is the classic namespace
    /// bug, so the distinction is a parameter rather than an
    /// assumption.
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

    fn peek_end_tag_name(&self) -> String {
        self.input[self.pos + 2..]
            .split(['>', ' ', '\t', '\n', '\r'])
            .next()
            .unwrap_or_default()
            .to_owned()
    }

    fn starts_with(&self, s: &str) -> bool {
        self.input[self.pos..].starts_with(s)
    }

    fn peek_is(&self, b: u8) -> bool {
        self.bytes.get(self.pos) == Some(&b)
    }

    fn skip_whitespace(&mut self) {
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
    fn expand_entity(&mut self, ent: &str, offset: usize) -> Result<String> {
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
                let mut budget = self.entity_budget;
                let out = self.expand_text(&text, offset, 1, &mut budget);
                self.entity_budget = budget;
                out
            }
            Some(crate::dtd::EntityValue::External) => Ok(String::new()),
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
    fn expand_text(
        &mut self,
        text: &str,
        offset: usize,
        depth: usize,
        budget: &mut usize,
    ) -> Result<String> {
        if depth > self.limits.max_entity_depth {
            return Err(Error::new(ErrorKind::EntityLimitExceeded, offset));
        }
        let mut out = String::new();
        let mut rest = text;
        while let Some(amp) = rest.find('&') {
            let (before, tail) = rest.split_at(amp);
            Self::push_bounded(&mut out, before, offset, budget)?;
            let Some(semi) = tail.find(';') else {
                return Err(Error::new(
                    ErrorKind::UnknownEntity(tail.to_owned()),
                    offset,
                ));
            };
            let name = &tail[1..semi];
            rest = &tail[semi + 1..];
            if let Some(direct) = decode_predefined(name) {
                Self::push_bounded(&mut out, &direct, offset, budget)?;
                continue;
            }
            let inner = match self.dtd.as_ref().and_then(|d| d.entity(name)) {
                Some(crate::dtd::EntityValue::Internal(t)) => t.clone(),
                _ => continue,
            };
            let expanded =
                self.expand_text(&inner, offset, depth + 1, budget)?;
            Self::push_bounded(&mut out, &expanded, offset, budget)?;
        }
        Self::push_bounded(&mut out, rest, offset, budget)?;
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
fn validate_xml_declaration(decl: &str, offset: usize) -> Result<()> {
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

    // `VersionInfo` is required, not optional.
    if seen.first() != Some(&"version") {
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
