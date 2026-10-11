// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! How a document is written: the options and their defaults.

/// How a document should be written.
///
/// The default is what [`Document::to_xml`](crate::Document::to_xml) does: no whitespace of any
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
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::Lf => "\n",
            Self::CrLf => "\r\n",
        }
    }
}
