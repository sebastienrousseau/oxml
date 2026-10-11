// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! What a reader yields: one event per construct.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::tree::ExpandedName;

/// One thing found in a document, owning its text.
///
/// For events that borrow character data from the reader's buffer
/// instead, see [`BorrowedEvent`].
///
/// A start tag carries everything needed to write it out again: the
/// resolved name, the prefix it was written with, its attributes and
/// the namespace declarations it made. An attribute's prefix is not
/// carried; a writer picks one bound to its namespace from the
/// declarations in scope, which is how the serialiser does it too.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Event {
    /// An element opened.
    ///
    /// A self-closing element yields this and then
    /// [`Event::EndElement`], so a caller counting depth need not
    /// treat it specially.
    #[non_exhaustive]
    StartElement {
        /// The element's expanded name.
        name: ExpandedName,
        /// The prefix the name was written with, if any.
        prefix: Option<String>,
        /// Its attributes, with names resolved and values expanded.
        ///
        /// Namespace declarations are not attributes here; they are in
        /// `namespaces`.
        attributes: Vec<(ExpandedName, String)>,
        /// The namespace declarations on this tag, in source order, as
        /// `(prefix, uri)`. `None` is the default namespace (`xmlns`).
        namespaces: Vec<(Option<String>, String)>,
    },
    /// An element closed.
    #[non_exhaustive]
    EndElement {
        /// The element's expanded name.
        name: ExpandedName,
        /// The prefix the name was written with, if any.
        prefix: Option<String>,
    },
    /// Character data, with entities expanded and `CDATA` merged in.
    ///
    /// One event per run: adjacent text, `CDATA` and references
    /// arrive together, exactly as they would become one text node.
    Text(String),
    /// A comment's content, without the `<!--` and `-->`.
    Comment(String),
    /// A processing instruction.
    ProcessingInstruction {
        /// The target, e.g. `xml-stylesheet`.
        target: String,
        /// Everything after the target, verbatim.
        data: String,
    },
}

/// One thing found in a document, borrowing from the reader's buffer
/// where it can.
///
/// Character data, attribute values, comments and processing
/// instructions are `Cow::Borrowed` slices of the input when they
/// appear in it verbatim, and `Cow::Owned` when something rewrote
/// them: an expanded entity, a `CDATA` section merged into the text
/// around it, a tokenized attribute collapsed. Names are owned: they
/// are resolved against the namespaces in scope, which the input does
/// not spell out.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum BorrowedEvent<'a> {
    /// An element opened. See [`Event::StartElement`].
    #[non_exhaustive]
    StartElement {
        /// The element's expanded name.
        name: ExpandedName,
        /// The prefix the name was written with, if any.
        prefix: Option<Cow<'a, str>>,
        /// Its attributes, with names resolved and values expanded.
        attributes: Vec<(ExpandedName, Cow<'a, str>)>,
        /// The namespace declarations on this tag, as `(prefix, uri)`.
        namespaces: Vec<(Option<String>, String)>,
    },
    /// An element closed.
    #[non_exhaustive]
    EndElement {
        /// The element's expanded name.
        name: ExpandedName,
        /// The prefix the name was written with, if any.
        prefix: Option<Cow<'a, str>>,
    },
    /// Character data, borrowed where contiguous or owned where rewritten.
    Text(Cow<'a, str>),
    /// A comment's content, without the `<!--` and `-->`.
    Comment(Cow<'a, str>),
    /// A processing instruction.
    ProcessingInstruction {
        /// The target, e.g. `xml-stylesheet`.
        target: Cow<'a, str>,
        /// Everything after the target, verbatim.
        data: Cow<'a, str>,
    },
}

impl BorrowedEvent<'_> {
    /// Convert this borrowed event into an owned [`Event`].
    #[must_use]
    pub fn into_owned(self) -> Event {
        match self {
            Self::StartElement {
                name,
                prefix,
                attributes,
                namespaces,
            } => Event::StartElement {
                name,
                prefix: prefix.map(Cow::into_owned),
                attributes: attributes
                    .into_iter()
                    .map(|(n, v)| (n, v.into_owned()))
                    .collect(),
                namespaces,
            },
            Self::EndElement { name, prefix } => Event::EndElement {
                name,
                prefix: prefix.map(Cow::into_owned),
            },
            Self::Text(text) => Event::Text(text.into_owned()),
            Self::Comment(comment) => Event::Comment(comment.into_owned()),
            Self::ProcessingInstruction { target, data } => {
                Event::ProcessingInstruction {
                    target: target.into_owned(),
                    data: data.into_owned(),
                }
            }
        }
    }
}

impl From<BorrowedEvent<'_>> for Event {
    fn from(b: BorrowedEvent<'_>) -> Self {
        b.into_owned()
    }
}
