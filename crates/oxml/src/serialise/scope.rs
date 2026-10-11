// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The namespace bindings in force while writing, and how a name
//! is resolved against them.

use alloc::borrow::Cow;
use alloc::vec::Vec;
use core::fmt::Write;

use super::escape::write_attribute_value;

/// The namespace bound to the `xml` prefix in every document.
const XML_NAMESPACE: &str = "http://www.w3.org/XML/1998/namespace";

/// The namespace bindings in force while writing, innermost last.
///
/// A binding is `(prefix, uri)`, with the empty prefix standing for
/// the default namespace. Prefixes borrow from the document except
/// the ones this writer invents, which is why they are `Cow`.
pub(super) struct Scope<'a> {
    bindings: Vec<(Cow<'a, str>, &'a str)>,
}

impl<'a> Scope<'a> {
    pub(super) fn new() -> Self {
        // `xml` is bound in every document by definition and is never
        // declared, so it is in scope before the root element.
        Self {
            bindings: Vec::from([(Cow::Borrowed("xml"), XML_NAMESPACE)]),
        }
    }

    pub(super) fn len(&self) -> usize {
        self.bindings.len()
    }

    pub(super) fn truncate(&mut self, len: usize) {
        self.bindings.truncate(len);
    }

    pub(super) fn push(&mut self, prefix: Cow<'a, str>, uri: &'a str) -> usize {
        self.bindings.push((prefix, uri));
        self.bindings.len() - 1
    }

    pub(super) fn prefix(&self, i: usize) -> &str {
        self.bindings.get(i).map_or("", |(p, _)| p.as_ref())
    }

    /// The innermost binding of `prefix`, as an index.
    pub(super) fn binding(&self, prefix: &str) -> Option<usize> {
        self.bindings
            .iter()
            .rposition(|(p, _)| p.as_ref() == prefix)
    }

    /// What `prefix` is bound to, if anything.
    pub(super) fn lookup(&self, prefix: &str) -> Option<&'a str> {
        self.binding(prefix).map(|i| self.bindings[i].1)
    }

    /// The binding of `prefix` if it is to `uri`, as an index.
    pub(super) fn bound_to(
        &self,
        prefix: Option<&str>,
        uri: &str,
    ) -> Option<usize> {
        let i = self.binding(prefix?)?;
        (self.bindings[i].1 == uri).then_some(i)
    }

    /// Whether `prefix` may be declared here without shadowing a
    /// binding in use.
    pub(super) fn is_free(&self, prefix: &str) -> bool {
        self.lookup(prefix).is_none_or(str::is_empty)
    }

    /// A non-default prefix currently bound to `uri`, as an index.
    pub(super) fn prefix_for(&self, uri: &str) -> Option<usize> {
        (0..self.bindings.len()).rev().find(|&i| {
            let (p, u) = &self.bindings[i];
            !p.is_empty() && *u == uri && self.lookup(p) == Some(*u)
        })
    }

    /// Bind `uri` to a fresh prefix: the stored one if it is free,
    /// otherwise `ns1`, `ns2`, ... until one is.
    pub(super) fn declare(
        &mut self,
        stored: Option<&'a str>,
        uri: &'a str,
    ) -> usize {
        if let Some(p) = stored.filter(|p| self.is_free(p)) {
            return self.push(Cow::Borrowed(p), uri);
        }
        let mut n = 1u32;
        loop {
            let candidate = alloc::format!("ns{n}");
            if self.is_free(&candidate) {
                return self.push(Cow::Owned(candidate), uri);
            }
            n += 1;
        }
    }
}

/// How a name is written: bare, or with the prefix at a scope index.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Written {
    Bare,
    Prefixed(usize),
}

/// A name in no namespace.
///
/// An unprefixed attribute is in no namespace whatever is in force.
/// An unprefixed element is in the default namespace, so one in no
/// namespace under a default must undeclare it.
pub(super) fn resolve_unqualified(
    attribute: bool,
    scope: &mut Scope<'_>,
) -> Written {
    if !attribute && scope.lookup("").is_some_and(|u| !u.is_empty()) {
        let _ = scope.push(Cow::Borrowed(""), "");
    }
    Written::Bare
}

/// A name in namespace `uri`, parsed with prefix `stored` if any.
///
/// The stored prefix is used when it is bound to `uri` here; failing
/// that, the default namespace for an element, then any prefix bound
/// to `uri`, and finally a declaration on this element.
pub(super) fn resolve_qualified<'a>(
    stored: Option<&'a str>,
    uri: &'a str,
    attribute: bool,
    scope: &mut Scope<'a>,
) -> Written {
    if let Some(i) = scope.bound_to(stored, uri) {
        return Written::Prefixed(i);
    }
    if !attribute && scope.lookup("") == Some(uri) {
        return Written::Bare;
    }
    let i = scope
        .prefix_for(uri)
        .unwrap_or_else(|| scope.declare(stored, uri));
    Written::Prefixed(i)
}

/// The declarations the writer added for this element: everything
/// pushed on the scope since `from`.
pub(super) fn write_auto_declarations<W: Write>(
    out: &mut W,
    scope: &Scope<'_>,
    from: usize,
) -> core::fmt::Result {
    for (prefix, uri) in &scope.bindings[from..] {
        write_declaration(out, prefix, uri)?;
    }
    Ok(())
}

pub(super) fn write_declaration<W: Write>(
    out: &mut W,
    prefix: &str,
    uri: &str,
) -> core::fmt::Result {
    if prefix.is_empty() {
        out.write_str(" xmlns=\"")?;
    } else {
        write!(out, " xmlns:{prefix}=\"")?;
    }
    write_attribute_value(uri, out)?;
    out.write_char('"')
}
