// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Writing a [`Document`] back out as XML.
//!
//! The contract -- what is guaranteed and what is not preserved -- is
//! documented on [`Document::to_xml`], where a caller will find it.
//!
//! Names are written through a scope of namespace bindings that the
//! writer keeps as it descends. A parsed document carries every
//! declaration it needs as namespace nodes, so the scope only confirms
//! the stored prefix is bound. A name the mutation API created has no
//! prefix and may be in a namespace nothing declares; the scope finds
//! a binding in force or declares one on the element, so what is
//! written always parses back to the same expanded names.

use alloc::borrow::Cow;
use alloc::string::String;
use core::fmt::Write;
#[cfg(feature = "std")]
use std::io;

use crate::tree::{Document, NameId, NodeId, NodeKind};

mod escape;
mod options;
mod scope;

pub use options::{EmptyElement, Indent, Newline, SerialiseOptions};

#[cfg(feature = "std")]
use escape::IoSink;
use escape::{is_c0_control, write_attribute_value, write_text};
use scope::{
    Scope, Written, resolve_qualified, resolve_unqualified,
    write_auto_declarations, write_declaration,
};

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
    /// A document built or changed through the mutation API parses
    /// back to the same expanded names: a name in a namespace nothing
    /// declares is given a declaration on its element, with the prefix
    /// it was parsed with where there was one and `ns1`, `ns2`, ...
    /// otherwise.
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
        self.write_xml_with(out, SerialiseOptions::default())
    }

    /// Write this document as XML into any [`std::io::Write`].
    ///
    /// The byte-oriented counterpart of [`Document::write_xml_with`],
    /// for a file or a socket. The output is UTF-8 and identical to
    /// [`Document::to_xml_with`] with the same options. The writer is
    /// flushed before returning.
    ///
    /// # Errors
    ///
    /// The first error the writer returned.
    #[cfg(feature = "std")]
    #[cfg_attr(docsrs, doc(cfg(feature = "std")))]
    pub fn write_xml_to<W: io::Write>(
        &self,
        w: W,
        options: &SerialiseOptions,
    ) -> io::Result<()> {
        let mut sink = IoSink {
            inner: w,
            error: None,
        };
        let outcome = self.write_xml_with(&mut sink, *options);
        match (outcome, sink.error) {
            (_, Some(e)) => Err(e),
            (Err(core::fmt::Error), None) => {
                Err(io::Error::other("formatting failed"))
            }
            (Ok(()), None) => sink.inner.flush(),
        }
    }

    /// Whether any content can only be expressed in XML 1.1.
    ///
    /// Two things require it, and the second is not about characters
    /// at all:
    ///
    /// - a C0 control character, which 1.0 forbids outright and 1.1
    ///   permits as a reference;
    /// - **unbinding a prefix** with `xmlns:p=""`, which Namespaces 1.1
    ///   allows and Namespaces 1.0 rejects as a reserved use.
    ///
    /// The C1 range `U+007F..U+009F` does **not**: XML 1.0 permits it
    /// literally and as a reference. Sharing one predicate with
    /// [`needs_reference`] declared every 1.0 document containing
    /// `U+0085` as 1.1, which is a different document.
    ///
    /// The corpus found the unbinding case after the first was fixed,
    /// which is the argument for running a property over 1,910 real
    /// documents rather than over the cases one thinks of.
    ///
    /// A namespace URI is written as an attribute value too, whether
    /// declared or given only in a name, so a control character in one
    /// needs 1.1 as much as one in a value does.
    fn needs_xml_11(&self) -> bool {
        self.nodes().any(|id| match self.kind(id) {
            Some(NodeKind::Text(t)) => t.chars().any(is_c0_control),
            Some(NodeKind::Attr(a)) => {
                a.value.chars().any(is_c0_control) || self.c0_in_uri(a.name)
            }
            Some(NodeKind::Element { name, .. }) => self.c0_in_uri(name),
            Some(NodeKind::Namespace { prefix, uri }) => {
                (!prefix.is_empty() && uri.is_empty())
                    || uri.chars().any(is_c0_control)
            }
            _ => false,
        })
    }

    /// Whether the namespace URI of `name` holds a C0 control.
    fn c0_in_uri(&self, name: crate::tree::NameId) -> bool {
        self.name(name)
            .and_then(|n| n.namespace.as_deref())
            .is_some_and(|uri| uri.chars().any(is_c0_control))
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
        #[cfg(feature = "tracing")]
        let _span = tracing::trace_span!("oxml.serialise").entered();

        // A declaration is written only when the content requires one.
        // The tree does not record which version it was parsed as, and
        // without a declaration the output is read as 1.0 -- so a 1.1
        // document containing `&#1;` serialised to a bare `&#1;`
        // produced something that would not parse at all.
        if self.needs_xml_11() {
            out.write_str("<?xml version=\"1.1\"?>")?;
            if options.indent.is_some() {
                out.write_str(options.newline.as_str())?;
            }
        }
        let mut scope = Scope::new();
        let top = self.children(self.root());
        for (i, &child) in top.iter().enumerate() {
            self.write_node(child, out, options, 0, &mut scope)?;
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

    fn write_node<'a, W: Write>(
        &'a self,
        id: NodeId,
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
        scope: &mut Scope<'a>,
    ) -> core::fmt::Result {
        match self.kind(id) {
            Some(NodeKind::Element { name, .. }) => {
                self.write_element(id, name, out, options, depth, scope)
            }
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

    fn write_element<'a, W: Write>(
        &'a self,
        id: NodeId,
        name: NameId,
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
        scope: &mut Scope<'a>,
    ) -> core::fmt::Result {
        let mark = scope.len();
        let how = self.write_start_tag(id, name, out, scope)?;
        let children = self.children(id);
        let result = if children.is_empty() {
            self.write_empty_end(name, how, out, options, scope)
        } else {
            out.write_char('>')?;
            self.write_children(children, out, options, depth, scope)?;
            out.write_str("</")?;
            self.write_qname(name, how, out, scope)?;
            out.write_char('>')
        };
        scope.truncate(mark);
        result
    }

    /// `<name`, the declarations and the attributes, without the `>`.
    ///
    /// Names are resolved before anything is written, because resolving
    /// one may add a declaration that must appear in this tag.
    fn write_start_tag<'a, W: Write>(
        &'a self,
        id: NodeId,
        name: NameId,
        out: &mut W,
        scope: &mut Scope<'a>,
    ) -> Result<Written, core::fmt::Error> {
        self.push_declared(id, scope);
        let auto_from = scope.len();
        let how = self.resolve_name(name, false, scope);
        for &attr in self.attribute_nodes(id) {
            if let Some(NodeKind::Attr(a)) = self.kind(attr) {
                let _ = self.resolve_name(a.name, true, scope);
            }
        }
        out.write_char('<')?;
        self.write_qname(name, how, out, scope)?;
        self.write_declared(id, out)?;
        write_auto_declarations(out, scope, auto_from)?;
        self.write_attributes(id, out, scope)?;
        Ok(how)
    }

    fn write_empty_end<W: Write>(
        &self,
        name: NameId,
        how: Written,
        out: &mut W,
        options: SerialiseOptions,
        scope: &Scope<'_>,
    ) -> core::fmt::Result {
        match options.empty_elements {
            EmptyElement::SelfClosing => out.write_str("/>"),
            EmptyElement::SelfClosingSpaced => out.write_str(" />"),
            EmptyElement::Expanded => {
                out.write_str("></")?;
                self.write_qname(name, how, out, scope)?;
                out.write_char('>')
            }
        }
    }

    fn write_children<'a, W: Write>(
        &'a self,
        children: &[NodeId],
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
        scope: &mut Scope<'a>,
    ) -> core::fmt::Result {
        // Indent only where every child is an element. Any text,
        // comment or PI child means the element's content is written
        // byte-for-byte as the default mode writes it: inserting
        // whitespace next to character data would change that data.
        if options.indent.is_some()
            && children.iter().all(|&c| self.is_element(c))
        {
            return self.write_indented(children, out, options, depth, scope);
        }
        // Inside mixed content no whitespace may be added, but the
        // empty-element spelling still applies -- `<b/>` versus
        // `<b />` is a difference between tags, not inside character
        // data. Recurse with indentation off rather than falling back
        // to a plain writer, which would silently drop every option
        // below this point.
        let compact = SerialiseOptions {
            indent: None,
            ..options
        };
        for &child in children {
            self.write_node(child, out, compact, depth, scope)?;
        }
        Ok(())
    }

    fn write_indented<'a, W: Write>(
        &'a self,
        children: &[NodeId],
        out: &mut W,
        options: SerialiseOptions,
        depth: usize,
        scope: &mut Scope<'a>,
    ) -> core::fmt::Result {
        for &child in children {
            Self::write_indent(out, options, depth + 1)?;
            self.write_node(child, out, options, depth + 1, scope)?;
        }
        Self::write_indent(out, options, depth)
    }

    /// Bring the element's own namespace nodes into scope.
    fn push_declared<'a>(&'a self, id: NodeId, scope: &mut Scope<'a>) {
        for &ns in self.namespace_nodes(id) {
            if let Some(NodeKind::Namespace { prefix, uri }) = self.kind(ns) {
                let _ = scope.push(Cow::Borrowed(prefix), uri);
            }
        }
    }

    /// Decide how `name` is written here, declaring a namespace in
    /// `scope` if nothing in force binds it.
    fn resolve_name<'a>(
        &'a self,
        name: NameId,
        attribute: bool,
        scope: &mut Scope<'a>,
    ) -> Written {
        let stored = self.prefix(name).filter(|p| !p.is_empty());
        match self.name(name).and_then(|n| n.namespace.as_deref()) {
            Some(uri) => resolve_qualified(stored, uri, attribute, scope),
            None => resolve_unqualified(attribute, scope),
        }
    }

    fn write_qname<W: Write>(
        &self,
        name: NameId,
        how: Written,
        out: &mut W,
        scope: &Scope<'_>,
    ) -> core::fmt::Result {
        if let Written::Prefixed(i) = how {
            out.write_str(scope.prefix(i))?;
            out.write_char(':')?;
        }
        out.write_str(self.name(name).map_or("", |n| n.local.as_str()))
    }

    /// The element's own `xmlns` declarations.
    ///
    /// `xml` is skipped: it is bound in every document by definition,
    /// and writing it back would be a redeclaration the specification
    /// forbids in some positions.
    fn write_declared<W: Write>(
        &self,
        id: NodeId,
        out: &mut W,
    ) -> core::fmt::Result {
        for &ns in self.namespace_nodes(id) {
            if let Some(NodeKind::Namespace { prefix, uri }) = self.kind(ns)
                && prefix != "xml"
            {
                write_declaration(out, prefix, uri)?;
            }
        }
        Ok(())
    }

    fn write_attributes<'a, W: Write>(
        &'a self,
        id: NodeId,
        out: &mut W,
        scope: &mut Scope<'a>,
    ) -> core::fmt::Result {
        for &attr in self.attribute_nodes(id) {
            if let Some(NodeKind::Attr(a)) = self.kind(attr) {
                // Resolved once already by `write_start_tag`, so this
                // finds the binding and declares nothing.
                let how = self.resolve_name(a.name, true, scope);
                out.write_char(' ')?;
                self.write_qname(a.name, how, out, scope)?;
                out.write_str("=\"")?;
                write_attribute_value(a.value, out)?;
                out.write_char('"')?;
            }
        }
        Ok(())
    }
}
