// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Errors, with source positions.

use alloc::string::String;
use core::fmt;

/// What went wrong, and where.
///
/// Every variant carries a byte offset into the input. Reporting the
/// offset rather than a line/column pair keeps the parser from having
/// to track line breaks on the hot path; [`Error::line_column`]
/// recovers the human-facing position on demand, which is only ever
/// needed when something has already failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error {
    /// What kind of problem this is.
    pub kind: ErrorKind,
    /// Byte offset into the input where the problem was detected.
    pub offset: usize,
}

/// The category of a parse failure.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ErrorKind {
    /// Input ended in the middle of a construct.
    UnexpectedEof,
    /// A close tag did not match the open tag it closes.
    MismatchedEndTag {
        /// The name from the open tag.
        expected: String,
        /// The name found in the close tag.
        found: String,
    },
    /// A close tag with no matching open tag.
    UnexpectedEndTag(String),
    /// A character that cannot start a name appeared where a name was
    /// required.
    InvalidName,
    /// An attribute value was not quoted.
    UnquotedAttributeValue,
    /// The same attribute name appeared twice on one element.
    DuplicateAttribute(String),
    /// An entity reference that is not defined.
    UnknownEntity(String),
    /// A namespace prefix was used without being declared.
    UnboundPrefix(String),
    /// Content appeared after the root element closed.
    TrailingContent,
    /// The document has no root element.
    NoRootElement,
    /// A construct was not terminated, e.g. a comment without `-->`.
    Unterminated(&'static str),
}

impl Error {
    pub(crate) const fn new(kind: ErrorKind, offset: usize) -> Self {
        Self { kind, offset }
    }

    /// Recover the 1-based line and column for this error's offset.
    ///
    /// Counts in `char`s rather than bytes so the column is what a
    /// person looking at the file would count.
    #[must_use]
    pub fn line_column(&self, input: &str) -> (usize, usize) {
        let upto = &input[..self.offset.min(input.len())];
        let line = upto.matches('\n').count() + 1;
        let column = upto
            .rsplit('\n')
            .next()
            .map_or(1, |l| l.chars().count() + 1);
        (line, column)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "at byte {}: ", self.offset)?;
        match &self.kind {
            ErrorKind::UnexpectedEof => f.write_str("input ended unexpectedly"),
            ErrorKind::MismatchedEndTag { expected, found } => {
                write!(f, "</{found}> closes <{expected}>")
            }
            ErrorKind::UnexpectedEndTag(n) => {
                write!(f, "</{n}> has no matching open tag")
            }
            ErrorKind::InvalidName => f.write_str("expected a name"),
            ErrorKind::UnquotedAttributeValue => {
                f.write_str("attribute value must be quoted")
            }
            ErrorKind::DuplicateAttribute(n) => {
                write!(f, "duplicate attribute {n}")
            }
            ErrorKind::UnknownEntity(n) => {
                write!(f, "unknown entity &{n};")
            }
            ErrorKind::UnboundPrefix(p) => {
                write!(f, "namespace prefix {p} is not declared")
            }
            ErrorKind::TrailingContent => {
                f.write_str("content after the root element")
            }
            ErrorKind::NoRootElement => {
                f.write_str("document has no root element")
            }
            ErrorKind::Unterminated(what) => {
                write!(f, "unterminated {what}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// A parse result.
pub type Result<T> = core::result::Result<T, Error>;
