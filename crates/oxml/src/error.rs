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
    /// Entity expansion exceeded a bound in [`Limits`].
    ///
    /// [`Limits`]: crate::Limits
    EntityLimitExceeded,
    /// The declaration names an XML version this parser does not
    /// implement.
    UnsupportedVersion,
    /// A character appeared that the `Char` production forbids.
    ///
    /// Most C0 control characters are illegal anywhere in an XML
    /// document, including inside comments and attribute values.
    IllegalCharacter(char),
    /// The bytes are not valid in the encoding the document declares,
    /// or the declared `EncName` is not legal per production 81.
    MalformedEncoding,
    /// The document declares an encoding this crate cannot decode.
    ///
    /// Distinct from [`ErrorKind::MalformedEncoding`]: the name is
    /// legal, the document may be perfectly well-formed, and a caller
    /// can decode it themselves and use [`crate::parse`].
    UnsupportedEncoding,
    /// The document type declaration is syntactically malformed.
    ///
    /// This is a well-formedness error, not a validity error: the
    /// grammar of a declaration binds every parser, whether or not it
    /// validates documents against the content models declared there.
    MalformedDtd(&'static str),
    /// Elements were nested more deeply than [`Limits::max_depth`].
    ///
    /// [`Limits::max_depth`]: crate::Limits::max_depth
    DepthLimitExceeded,
    /// More attributes on one element than the limit allows.
    TooManyAttributes,
    /// An attribute value longer than the limit allows.
    AttributeTooLarge,
    /// A name longer than the limit allows.
    NameTooLong,
    /// More nodes in the document than the limit allows.
    TooManyNodes,
    /// A text or CDATA node longer than the limit allows.
    TextTooLong,
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
            ErrorKind::DepthLimitExceeded => {
                f.write_str("elements nested past the depth limit")
            }
            ErrorKind::TooManyAttributes => {
                f.write_str("too many attributes on one element")
            }
            ErrorKind::AttributeTooLarge => {
                f.write_str("attribute value exceeds the size limit")
            }
            ErrorKind::NameTooLong => {
                f.write_str("name exceeds the length limit")
            }
            ErrorKind::TooManyNodes => {
                f.write_str("document exceeds the node limit")
            }
            ErrorKind::TextTooLong => {
                f.write_str("text node exceeds the length limit")
            }
            ErrorKind::Unterminated(what) => {
                write!(f, "unterminated {what}")
            }
            ErrorKind::EntityLimitExceeded => {
                f.write_str("entity expansion exceeds the limit")
            }
            ErrorKind::UnsupportedVersion => {
                f.write_str("unsupported XML version")
            }
            ErrorKind::IllegalCharacter(c) => {
                write!(f, "character U+{:04X} is not allowed in XML", *c as u32)
            }
            ErrorKind::MalformedEncoding => {
                f.write_str("bytes are not valid in the declared encoding")
            }
            ErrorKind::UnsupportedEncoding => {
                f.write_str("declared encoding is not supported")
            }
            ErrorKind::MalformedDtd(why) => {
                write!(f, "malformed doctype: {why}")
            }
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// A parse result.
pub type Result<T> = core::result::Result<T, Error>;
