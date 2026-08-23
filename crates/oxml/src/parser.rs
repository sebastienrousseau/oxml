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
    /// [`crate::MAX_DEPTH`] so the recursion cannot exhaust the stack.
    depth: usize,
}

/// Parse an XML document.
///
/// # Errors
///
/// Returns [`Error`] if the input is not well-formed, or uses a
/// namespace prefix that was never declared.
pub fn parse(input: &str) -> Result<Document> {
    let mut p = Parser {
        input,
        bytes: input.as_bytes(),
        pos: 0,
        doc: Document::new(),
        ns: Namespaces::default(),
        depth: 0,
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
        if self.starts_with("<?xml") {
            let end = self.input[self.pos..].find("?>").ok_or_else(|| {
                Error::new(ErrorKind::Unterminated("XML declaration"), self.pos)
            })?;
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
        let start = self.pos;
        self.pos += "<!DOCTYPE".len();
        let mut depth = 0usize;
        while self.pos < self.bytes.len() {
            match self.bytes[self.pos] {
                b'[' => depth += 1,
                b']' => depth = depth.saturating_sub(1),
                b'>' if depth == 0 => {
                    self.pos += 1;
                    return Ok(());
                }
                _ => {}
            }
            self.pos += 1;
        }
        Err(Error::new(ErrorKind::Unterminated("doctype"), start))
    }

    fn parse_element(&mut self, parent: NodeId) -> Result<()> {
        // Checked on entry so the frame that would overflow is never
        // pushed. See `crate::MAX_DEPTH`.
        if self.depth >= crate::MAX_DEPTH {
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
            if self.pos >= self.bytes.len() {
                return Err(Error::new(ErrorKind::UnexpectedEof, self.pos));
            }
            if self.peek_is(b'<') {
                if self.starts_with("</") {
                    self.flush_text(&mut text, node);
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
                    self.flush_text(&mut text, node);
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
                    self.flush_text(&mut text, node);
                    let (t, d) = self.parse_pi()?;
                    let _ = self.doc.push(
                        NodeKind::ProcessingInstruction { target: t, data: d },
                        node,
                    );
                } else {
                    self.flush_text(&mut text, node);
                    self.parse_element(node)?;
                }
            } else {
                self.parse_text_run(&mut text)?;
            }
        }
    }

    fn flush_text(&mut self, text: &mut String, node: NodeId) {
        if !text.is_empty() {
            let owned = core::mem::take(text);
            let _ = self.doc.push(NodeKind::Text(owned), node);
        }
    }

    fn parse_text_run(&mut self, out: &mut String) -> Result<()> {
        while self.pos < self.bytes.len() {
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
                    out.push_str(&decode_entity(ent, s)?);
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
        let body = self.input[self.pos..self.pos + end].to_owned();
        self.pos += end + 3;
        Ok(body)
    }

    fn parse_pi(&mut self) -> Result<(String, String)> {
        let start = self.pos;
        self.pos += 2; // '<?'
        let target = self.parse_name()?;
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
            out.push((name, value));
        }
    }

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
                b'&' => {
                    let s = self.pos;
                    self.pos += 1;
                    let end =
                        self.input[self.pos..].find(';').ok_or_else(|| {
                            Error::new(ErrorKind::Unterminated("entity"), s)
                        })?;
                    let ent = &self.input[self.pos..self.pos + end];
                    out.push_str(&decode_entity(ent, s)?);
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
        let start = self.pos;
        let rest = &self.input[self.pos..];
        let mut chars = rest.char_indices();
        match chars.next() {
            Some((_, c)) if is_name_start(c) => {}
            _ => {
                return Err(Error::new(ErrorKind::InvalidName, start));
            }
        }
        let mut end = rest.len();
        for (i, c) in rest.char_indices() {
            if !is_name_char(c) {
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
}

/// Resolve one entity reference.
///
/// Only the five predefined entities and numeric character references
/// are supported. External and custom entities are deliberately *not*
/// resolved: that is the XXE and billion-laughs attack surface, and a
/// parser that cannot expand them cannot be made to leak a file or
/// exhaust memory through one.
fn decode_entity(ent: &str, offset: usize) -> Result<String> {
    let out = match ent {
        "lt" => "<".to_owned(),
        "gt" => ">".to_owned(),
        "amp" => "&".to_owned(),
        "apos" => "'".to_owned(),
        "quot" => "\"".to_owned(),
        _ => {
            let cp = if let Some(hex) = ent.strip_prefix("#x") {
                u32::from_str_radix(hex, 16).ok()
            } else if let Some(dec) = ent.strip_prefix('#') {
                dec.parse::<u32>().ok()
            } else {
                None
            };
            let ch = cp.and_then(char::from_u32).ok_or_else(|| {
                Error::new(ErrorKind::UnknownEntity(ent.to_owned()), offset)
            })?;
            let mut s = String::new();
            s.push(ch);
            s
        }
    };
    Ok(out)
}

fn is_name_start(c: char) -> bool {
    matches!(c, 'A'..='Z' | 'a'..='z' | '_' | ':')
        || matches!(c as u32,
            0xC0..=0xD6 | 0xD8..=0xF6 | 0xF8..=0x2FF
            | 0x370..=0x37D | 0x37F..=0x1FFF | 0x200C..=0x200D
            | 0x2070..=0x218F | 0x2C00..=0x2FEF | 0x3001..=0xD7FF
            | 0xF900..=0xFDCF | 0xFDF0..=0xFFFD | 0x10000..=0xEFFFF)
}

fn is_name_char(c: char) -> bool {
    is_name_start(c)
        || matches!(c, '-' | '.' | '0'..='9')
        || c as u32 == 0xB7
        || matches!(c as u32, 0x300..=0x36F | 0x203F..=0x2040)
}
