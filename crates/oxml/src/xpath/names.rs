// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Names and languages of nodes, for `local-name()`, `name()`,
//! `namespace-uri()` and `lang()`.

use alloc::borrow::ToOwned;
use alloc::string::String;

use crate::tree::{Document, NodeId, NodeKind};

/// The expanded name of a node, as `local-name` and `namespace-uri`
/// define it.
///
/// `XPath` 1.0 gives an expanded-name to elements **and attributes**;
/// reading only `Document::element_name` here meant both functions
/// answered the empty string for every attribute, which silently broke
/// the one workaround available for selecting by namespace. A
/// processing instruction has a local part -- its target -- and no
/// namespace. Everything else has neither.
pub(crate) fn name_parts(
    doc: &Document,
    id: NodeId,
) -> Option<(&str, Option<&str>)> {
    match doc.kind(id)? {
        NodeKind::Element { .. } => doc
            .element_name(id)
            .map(|n| (n.local.as_str(), n.namespace.as_deref())),
        NodeKind::Attr(attribute) => doc
            .name(attribute.name)
            .map(|n| (n.local.as_str(), n.namespace.as_deref())),
        NodeKind::ProcessingInstruction { target, .. } => Some((target, None)),
        // The node's *name* is the prefix; the URI is its
        // string-value, not its namespace. A namespace node is itself
        // in no namespace.
        NodeKind::Namespace { prefix, .. } => Some((prefix, None)),
        NodeKind::Root | NodeKind::Text(_) | NodeKind::Comment(_) => None,
    }
}

/// The namespace `xml:` is bound to by specification, and so the one
/// `xml:lang` carries once names are resolved.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The value of an attribute on a node, by expanded name, without
/// building the intermediate `Vec` that [`Document::attributes`]
/// returns.
fn attribute_by_name<'d>(
    doc: &'d Document,
    id: NodeId,
    namespace: Option<&str>,
    local: &str,
) -> Option<&'d str> {
    doc.attribute_nodes(id).iter().find_map(|&a| {
        let NodeKind::Attr(attribute) = doc.kind(a)? else {
            return None;
        };
        let name = doc.name(attribute.name)?;
        (name.local == local && name.namespace.as_deref() == namespace)
            .then_some(attribute.value)
    })
}

/// The `xml:lang` in scope for a node: the one on the node itself, or
/// failing that the one on its nearest ancestor that carries it.
///
/// `xml:lang` is defined to be inherited, so reading only the context
/// node would answer `false` for every element below the one that
/// declares the language -- which is nearly always where the text is.
fn in_scope_lang(doc: &Document, id: NodeId) -> Option<&str> {
    let mut at = id;
    loop {
        if let Some(value) =
            attribute_by_name(doc, at, Some(XML_NAMESPACE), "lang")
        {
            return Some(value);
        }
        at = doc.parent(at)?;
    }
}

/// `lang(s)`: whether the in-scope language is `s`, or is `s` followed
/// by `-` and a subtag, so `lang("en")` is true for `en-GB`.
///
/// Language tags are ASCII and compared without regard to case.
pub(crate) fn lang_matches(doc: &Document, ctx: NodeId, want: &str) -> bool {
    let Some(have) = in_scope_lang(doc, ctx) else {
        return false;
    };
    if have.eq_ignore_ascii_case(want) {
        return true;
    }
    // A suffix only counts when a `-` separates it, so `lang("e")` is
    // false for `en`. Taking the prefix with `get` rather than slicing
    // keeps a multi-byte character straddling the boundary from
    // panicking, though the `-` test already implies a boundary.
    have.as_bytes().get(want.len()) == Some(&b'-')
        && have
            .get(..want.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(want))
}

/// The `QName` `name()` reports: the prefix the name was written
/// with, a colon, then the local part -- or just the local part when
/// the name had no prefix.
///
/// A processing instruction's name is its target, which never has a
/// prefix. Everything without an expanded name answers the empty
/// string, as `local-name` does.
pub(crate) fn qualified_name(doc: &Document, id: NodeId) -> Option<String> {
    let name_id = match doc.kind(id)? {
        NodeKind::Element { name, .. } => name,
        NodeKind::Attr(attribute) => attribute.name,
        NodeKind::ProcessingInstruction { target, .. } => {
            return Some(target.to_owned());
        }
        NodeKind::Namespace { prefix, .. } => return Some(prefix.to_owned()),
        NodeKind::Root | NodeKind::Text(_) | NodeKind::Comment(_) => {
            return None;
        }
    };
    let local = &doc.name(name_id)?.local;
    Some(match doc.prefix(name_id) {
        Some(prefix) => {
            let mut out = String::with_capacity(prefix.len() + 1 + local.len());
            out.push_str(prefix);
            out.push(':');
            out.push_str(local);
            out
        }
        None => local.clone(),
    })
}
