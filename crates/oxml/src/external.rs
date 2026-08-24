// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Content for external entities and subsets, supplied by the caller.
//!
//! oxml never performs I/O. A document that references an external
//! entity or an external DTD subset names a *location*, and resolving
//! that location is the caller's decision -- they have the permission
//! model, the user, and the context to make it. See
//! [ADR 0003](https://github.com/sebastienrousseau/oxml/blob/main/doc/adr/0003-no-external-entities.md).
//!
//! Without a source, an external reference expands to nothing and the
//! declarations in an external subset are unknown, which is what
//! [`crate::parse`] does. With one, the same parse can check the rules
//! that only the external content can settle.

/// Somewhere the caller can look up external content.
///
/// Implemented for `&[(&str, &str)]`, which is enough for a test
/// fixture or a document whose parts are already in memory.
///
/// # Examples
///
/// ```
/// use oxml::{Limits, external::ExternalSource, parse_with_external};
///
/// // A slice of (system identifier, content) pairs is a source.
/// let parts: &[(&str, &str)] = &[("greeting.ent", "hello")];
/// assert_eq!(parts.fetch("greeting.ent", None), Some("hello"));
///
/// let doc = parse_with_external(
///     r#"<!DOCTYPE d [<!ENTITY g SYSTEM "greeting.ent">]><d>&g;</d>"#,
///     Limits::default(),
///     &parts,
/// )?;
/// assert_eq!(doc.text(doc.root()), "hello");
/// # Ok::<(), oxml::Error>(())
/// ```
pub trait ExternalSource {
    /// The content for an identifier, or `None` if it is unavailable.
    ///
    /// Returning `None` is not an error: it means the same thing as
    /// having no source at all for that identifier, so a caller can
    /// supply the parts they have and leave the rest.
    fn fetch(&self, system_id: &str, public_id: Option<&str>) -> Option<&str>;
}

impl ExternalSource for [(&str, &str)] {
    fn fetch(&self, system_id: &str, _public: Option<&str>) -> Option<&str> {
        self.iter()
            .find(|(id, _)| *id == system_id)
            .map(|(_, content)| *content)
    }
}

impl<T: ExternalSource + ?Sized> ExternalSource for &T {
    fn fetch(&self, system_id: &str, public: Option<&str>) -> Option<&str> {
        (**self).fetch(system_id, public)
    }
}

/// Nothing is available.
///
/// The behaviour of [`crate::parse`], expressed as a source so that one
/// code path serves both.
pub(crate) struct NoExternal;

impl ExternalSource for NoExternal {
    fn fetch(&self, _system: &str, _public: Option<&str>) -> Option<&str> {
        None
    }
}
