// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Writing a [`Document`] back out as XML.
//!
//! The contract -- what is guaranteed and what is not preserved -- is
//! documented on [`Document::to_xml`], where a caller will find it.

use alloc::string::String;
use core::fmt::Write;

use crate::tree::{Document, NodeId, NodeKind};

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

        for &ns in self.namespace_nodes(id) {
            if let Some(NodeKind::Namespace { prefix, uri }) = self.kind(ns) {
                // `xml` is bound in every document by definition.
                // Writing it back would be a redeclaration the
                // specification forbids in some positions and that no
                // parser needs.
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
