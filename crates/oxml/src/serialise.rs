// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Writing a [`Document`] back out as XML.
//!
//! The contract -- what is guaranteed and what is not preserved -- is
//! documented on [`Document::to_xml`], where a caller will find it.

use alloc::string::String;
use core::fmt::Write;

use crate::tree::{Document, NodeId, NodeKind};

/// How a document should be written.
///
/// The default is what [`Document::to_xml`] does: no whitespace of any
/// kind added, `<a/>` for an empty element. That is the only mode in
/// which serialisation is a fixed point over the conformance corpus,
/// and it stays the default because the alternative -- a default that
/// edits documents -- would have to be discovered rather than chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SerialiseOptions {
    /// Indentation, or `None` to add no whitespace at all.
    ///
    /// Indentation is only inserted between children of an element
    /// whose children are **all elements**. An element with any text,
    /// CDATA-derived or comment child is written exactly as the
    /// default mode writes it, because inserting whitespace there
    /// changes the document's text -- `<p>a<b/>c</p>` pretty-printed
    /// naively contains different character data than it did before.
    /// This makes pretty-printing safe *by construction* rather than
    /// by a warning in the documentation.
    pub indent: Option<Indent>,
    /// How an element with no children is written.
    pub empty_elements: EmptyElement,
    /// The line ending used when `indent` is set.
    pub newline: Newline,
}

/// One level of indentation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Indent {
    /// This many spaces per depth level.
    Spaces(u8),
    /// One tab per depth level.
    Tab,
}

/// How `<a></a>` with nothing inside is spelled.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EmptyElement {
    /// `<a/>` -- the default, and what `to_xml` has always written.
    #[default]
    SelfClosing,
    /// `<a />`, the XHTML-compatibility spelling.
    SelfClosingSpaced,
    /// `<a></a>`.
    Expanded,
}

/// The line ending between elements when indenting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Newline {
    /// `\n` -- the default.
    #[default]
    Lf,
    /// `\r\n`.
    CrLf,
}

impl Newline {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}

impl Document {
    /// Write this document as XML.
    ///
    /// # What is guaranteed
    ///
    /// Serialisation is **idempotent over parsing**: for any document
    /// that parses,
    ///
    /// ```text
    /// to_xml(parse(to_xml(parse(s))))  ==  to_xml(parse(s))
    /// ```
    ///
    /// The first pass may change the text -- expanding an entity,
    /// dropping a CDATA wrapper -- and no pass after it changes
    /// anything. The output always parses, and parses to a tree with
    /// the same elements, attributes, text, comments and processing
    /// instructions in the same order. Every document in the W3C
    /// conformance corpus that this parser accepts is checked against
    /// that property.
    ///
    /// # What is not preserved
    ///
    /// This is **not** a byte-exact round trip, and cannot be: the
    /// tree does not retain what one would need.
    ///
    /// - **Entity references are expanded at parse.** `&amp;`, `&#38;`
    ///   and a general entity expanding to `&` all become the same
    ///   character, and nothing records which was written.
    /// - **CDATA sections become text.**
    /// - **Quoting style is not kept**; values come back double-quoted.
    /// - **`<a/>` and `<a></a>` are the same element**, and an element
    ///   with no children is written self-closing.
    /// - **Whitespace outside the root, and any DOCTYPE, are dropped.**
    ///
    /// Each is a lexical difference over identical content, which is
    /// what makes the guarantee above achievable instead.
    #[must_use]
    pub fn to_xml(&self) -> String {
        let mut out = String::new();
        // Writing into a `String` cannot fail; the `Result` exists for
        // the generic writer below.
        let _ = self.write_xml(&mut out);
        out
    }

    /// Write this document as XML into any [`core::fmt::Write`].
    ///
    /// # Errors
    ///
    /// Whatever the writer returns.
    pub fn write_xml<W: Write>(&self, out: &mut W) -> core::fmt::Result {
        // A declaration is written only when the content requires one.
        //
        // XML 1.1 permits C0 and C1 control characters as references;
        // 1.0 forbids them outright. The tree does not record which
        // version it was parsed as, and without a declaration the
        // output is read as 1.0 -- so a 1.1 document containing `&#1;`
        // serialised to a bare `&#1;` produced something that would not
        // parse at all.
        //
        // The W3C corpus caught this on five documents. Declaring 1.1
        // exactly when a character needs it keeps the output
        // self-describing without inventing a declaration for the
        // documents that do not.
        if self.needs_xml_11() {
            out.write_str("<?xml version=\"1.1\"?>")?;
        }
        for &child in self.children(self.root()) {
            self.write_node(child, out)?;
        }
        Ok(())
    }

    /// Whether any content can only be expressed in XML 1.1.
    ///
    /// Two things require it, and the second is not about characters
    /// at all:
    ///
    /// - a C0 or C1 control character, which 1.0 forbids outright and
    ///   1.1 permits as a reference;
    /// - **unbinding a prefix** with `xmlns:p=""`, which Namespaces 1.1
    ///   allows and Namespaces 1.0 rejects as a reserved use.
    ///
    /// The corpus found the second after the first was fixed, which is
    /// the argument for running a property over 1,910 real documents
    /// rather than over the cases one thinks of.
    fn needs_xml_11(&self) -> bool {
        self.descendants().any(|id| match self.kind(id) {
            Some(NodeKind::Text(t)) => t.chars().any(needs_reference),
            Some(NodeKind::Attr(a)) => a.value.chars().any(needs_reference),
            Some(NodeKind::Namespace { prefix, uri }) => {
                !prefix.is_empty() && uri.is_empty()
            }
            _ => false,
        })
    }

    /// The document as XML, formatted according to `options`.
    ///
    /// With `SerialiseOptions::default()` this is exactly
    /// [`Document::to_xml`]. With indentation set, whitespace is added
    /// only between children of elements whose children are all
    /// elements -- see [`SerialiseOptions::indent`] for why that
    /// restriction is what makes pretty-printing safe.
    ///
    /// A pretty-printed document is **not** guaranteed equal to its
    /// source, and reparsing it yields a tree containing the inserted
    /// whitespace as text nodes. The fixed-point guarantee belongs to
    /// the default options alone.
    #[must_use]
    pub fn to_xml_with(&self, options: SerialiseOptions) -> String {
        let mut out = String::new();
        let _ = self.write_xml_with(&mut out, options);
        out
    }

    /// Write this document as XML into any writer, with options.
    ///
    /// # Errors
    ///
    /// Whatever the writer returns.
    pub fn write_xml_with<W: Write>(
        &self,
        out: &mut W,
        options: SerialiseOptions,
    ) -> core::fmt::Result {
        if self.needs_xml_11() {
            out.write_str("<?xml version=\"1.1\"?>")?;
            if options.indent.is_some() {
                out.write_str(options.newline.as_str())?;
            }
        }
        let top = self.children(self.root());
        for (i, &child) in top.iter().enumerate() {
            self.write_node_with(child, out, options, 0)?;
            // A newline between top-level items (the root element and
            // any comments or PIs beside it), but not after the last.
            if options.indent.is_some() && i + 1 < top.len() {
                out.write_str(options.newline.as_str())?;
            }
        }
        Ok(())
    }

    fn write_indent<W: Write>(
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
    ) -> core::fmt::Result {
        out.write_str(options.newline.as_str())?;
        match options.indent {
            Some(Indent::Spaces(n)) => {
                for _ in 0..(depth * n as usize) {
                    out.write_char(' ')?;
                }
            }
            Some(Indent::Tab) => {
                for _ in 0..depth {
                    out.write_char('\t')?;
                }
            }
            None => {}
        }
        Ok(())
    }

    fn write_node_with<W: Write>(
        &self,
        id: NodeId,
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
    ) -> core::fmt::Result {
        match self.kind(id) {
            Some(NodeKind::Element { .. }) => {
                self.write_element_with(id, out, options, depth)
            }
            _ => self.write_node(id, out),
        }
    }

    fn write_element_with<W: Write>(
        &self,
        id: NodeId,
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
    ) -> core::fmt::Result {
        let name = self.qualified_name(id);
        write!(out, "<{name}")?;
        self.write_tag_attributes(id, out)?;

        let children = self.children(id);
        if children.is_empty() {
            return match options.empty_elements {
                EmptyElement::SelfClosing => out.write_str("/>"),
                EmptyElement::SelfClosingSpaced => out.write_str(" />"),
                EmptyElement::Expanded => write!(out, "></{name}>"),
            };
        }
        out.write_char('>')?;

        // Indent only where every child is an element. Any text,
        // comment or PI child means the element's content is written
        // byte-for-byte as the default mode writes it: inserting
        // whitespace next to character data would change that data.
        let element_only = options.indent.is_some()
            && children.iter().all(|&c| {
                matches!(self.kind(c), Some(NodeKind::Element { .. }))
            });

        if element_only {
            for &child in children {
                Self::write_indent(out, options, depth + 1)?;
                self.write_node_with(child, out, options, depth + 1)?;
            }
            Self::write_indent(out, options, depth)?;
        } else {
            // Inside mixed content no whitespace may be added, but the
            // empty-element spelling still applies -- `<b/>` versus
            // `<b />` is a difference between tags, not inside
            // character data. Recurse with indentation off rather
            // than falling back to the plain writer, which would
            // silently drop every option below this point.
            let compact = SerialiseOptions {
                indent: None,
                ..options
            };
            for &child in children {
                self.write_node_with(child, out, compact, depth)?;
            }
        }
        write!(out, "</{name}>")
    }

    /// Namespace declarations and attributes, shared by both writers.
    fn write_tag_attributes<W: Write>(
        &self,
        id: NodeId,
        out: &mut W,
    ) -> core::fmt::Result {
        for &ns in self.namespace_nodes(id) {
            if let Some(NodeKind::Namespace { prefix, uri }) = self.kind(ns) {
                if prefix == "xml" {
                    continue;
                }
                if prefix.is_empty() {
                    write!(out, " xmlns=\"")?;
                } else {
                    write!(out, " xmlns:{prefix}=\"")?;
                }
                write_attribute_value(uri, out)?;
                out.write_char('"')?;
            }
        }
        for &attr in self.attribute_nodes(id) {
            if let Some(NodeKind::Attr(a)) = self.kind(attr) {
                let attr_name = self.qualified_attribute_name(attr);
                write!(out, " {attr_name}=\"")?;
                write_attribute_value(a.value, out)?;
                out.write_char('"')?;
            }
        }
        Ok(())
    }

    fn write_node<W: Write>(
        &self,
        id: NodeId,
        out: &mut W,
    ) -> core::fmt::Result {
        match self.kind(id) {
            Some(NodeKind::Element { .. }) => self.write_element(id, out),
            Some(NodeKind::Text(t)) => write_text(t, out),
            Some(NodeKind::Comment(c)) => write!(out, "<!--{c}-->"),
            Some(NodeKind::ProcessingInstruction { target, data }) => {
                if data.is_empty() {
                    write!(out, "<?{target}?>")
                } else {
                    write!(out, "<?{target} {data}?>")
                }
            }
            // Attributes and namespaces are written by their element,
            // and the root has no representation of its own.
            _ => Ok(()),
        }
    }

    fn write_element<W: Write>(
        &self,
        id: NodeId,
        out: &mut W,
    ) -> core::fmt::Result {
        let name = self.qualified_name(id);
        write!(out, "<{name}")?;
        // `xml` is skipped inside: it is bound in every document by
        // definition, and writing it back would be a redeclaration the
        // specification forbids in some positions.
        self.write_tag_attributes(id, out)?;

        let children = self.children(id);
        if children.is_empty() {
            return out.write_str("/>");
        }
        out.write_char('>')?;
        for &child in children {
            self.write_node(child, out)?;
        }
        write!(out, "</{name}>")
    }

    /// An element's name as it should be written, prefix included.
    fn qualified_name(&self, id: NodeId) -> String {
        let mut s = String::new();
        if let Some(NodeKind::Element { name, .. }) = self.kind(id) {
            if let Some(prefix) = self.prefix(name) {
                if !prefix.is_empty() {
                    s.push_str(prefix);
                    s.push(':');
                }
            }
            if let Some(expanded) = self.name(name) {
                s.push_str(&expanded.local);
            }
        }
        s
    }

    /// An attribute's name as it should be written.
    fn qualified_attribute_name(&self, id: NodeId) -> String {
        let mut s = String::new();
        if let Some(NodeKind::Attr(a)) = self.kind(id) {
            if let Some(prefix) = self.prefix(a.name) {
                if !prefix.is_empty() {
                    s.push_str(prefix);
                    s.push(':');
                }
            }
            if let Some(expanded) = self.name(a.name) {
                s.push_str(&expanded.local);
            }
        }
        s
    }
}

/// Escape character data.
///
/// `>` is escaped even though only `]]>` requires it. Escaping it
/// unconditionally costs three characters and removes the need to track
/// whether the two preceding characters were `]]`.
///
/// A carriage return is written as `&#13;` rather than literally.
/// Parsing normalises a literal `\r` to `\n`, so writing one loses it
/// on the next read -- which is how the W3C corpus caught this:
/// `ibm14v02.xml` holds `\r` in its character data and serialising it
/// twice produced two different documents.
///
/// Control characters go out as references for the same reason from
/// the other direction: XML 1.1 permits them only in that form, so
/// writing one literally produces a document that will not parse.
fn write_text<W: Write>(text: &str, out: &mut W) -> core::fmt::Result {
    for c in text.chars() {
        match c {
            '&' => out.write_str("&amp;")?,
            '<' => out.write_str("&lt;")?,
            '>' => out.write_str("&gt;")?,
            '\r' => out.write_str("&#13;")?,
            c if needs_reference(c) => write_reference(c, out)?,
            other => out.write_char(other)?,
        }
    }
    Ok(())
}

/// Whether a character can only be written as a reference.
///
/// Tab and newline are legal literally and are left alone; everything
/// else below `0x20`, plus the C1 range that XML 1.1 also restricts,
/// has to be a reference or the output will not parse.
const fn needs_reference(c: char) -> bool {
    matches!(c, '\u{1}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}'
        | '\u{7f}'..='\u{9f}')
}

/// Write one character as a numeric reference.
fn write_reference<W: Write>(c: char, out: &mut W) -> core::fmt::Result {
    write!(out, "&#{};", c as u32)
}

/// Escape an attribute value.
///
/// Tab, newline and carriage return are written as character
/// references rather than literally. Attribute-value normalisation
/// turns a literal one into a space when the document is read back, so
/// writing them literally would lose them -- silently, and only for
/// values that happened to contain whitespace.
fn write_attribute_value<W: Write>(
    value: &str,
    out: &mut W,
) -> core::fmt::Result {
    for c in value.chars() {
        match c {
            '&' => out.write_str("&amp;")?,
            '<' => out.write_str("&lt;")?,
            '>' => out.write_str("&gt;")?,
            '"' => out.write_str("&quot;")?,
            '\t' => out.write_str("&#9;")?,
            '\n' => out.write_str("&#10;")?,
            '\r' => out.write_str("&#13;")?,
            c if needs_reference(c) => write_reference(c, out)?,
            other => out.write_char(other)?,
        }
    }
    Ok(())
}
