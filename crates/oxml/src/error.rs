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
    /// A reference the specification forbids in the place it appears.
    ///
    /// An external parsed entity may not be referenced in an attribute
    /// value (`WFC: No External Entity References`), and an unparsed
    /// entity may not be referenced anywhere (`WFC: Parsed Entity`).
    /// Distinct from [`ErrorKind::UnknownEntity`]: the entity is
    /// declared, and the declaration is the problem.
    ForbiddenEntityReference(String),
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
    /// `]]>` appeared literally in character data.
    IllegalCdataEnd,
    /// A reserved namespace prefix or URI was misused.
    ReservedNamespace,
    /// A comment contains `--`, or ends with `-`.
    MalformedComment,
    /// The XML declaration does not match its grammar.
    MalformedDeclaration,
    /// A processing instruction used the reserved target `xml`.
    ReservedPiTarget,
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
    /// The byte source failed while the document was being read.
    ///
    /// Only [`crate::stream::Reader::from_reader`] can produce this:
    /// every other entry point is handed text that has already been
    /// read. It carries the reader's own message, because a caller
    /// debugging a truncated file needs to know whether the file was
    /// truncated or the disk was busy.
    Io(alloc::string::String),
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
    /// A single construct grew past [`Limits::max_construct_bytes`]
    /// before its end arrived.
    ///
    /// Only the incremental readers produce this: a start tag, comment
    /// or run of character data is buffered until it is whole, and a
    /// source that never closes one would otherwise be buffered until
    /// memory ran out. The offset is where the construct began.
    ///
    /// [`Limits::max_construct_bytes`]: crate::Limits::max_construct_bytes
    ConstructTooLarge,
    /// The document type declaration, with everything it pulled in,
    /// is longer than [`Limits::max_dtd_bytes`] allows.
    ///
    /// [`Limits::max_dtd_bytes`]: crate::Limits::max_dtd_bytes
    DtdTooLarge,
    /// DTD groups or included sections exceed [`Limits::max_dtd_depth`](crate::Limits::max_dtd_depth).
    DtdDepthLimitExceeded,
    /// Source bytes exceed [`Limits::max_input_bytes`](crate::Limits::max_input_bytes).
    InputTooLarge,
    /// Decoded bytes exceed [`Limits::max_document_bytes`](crate::Limits::max_document_bytes).
    DocumentTooLarge,
}

impl Error {
    pub(crate) fn new(kind: ErrorKind, offset: usize) -> Self {
        // The DTD parser reports everything as an offset and a static
        // reason, limits included, and arrives here as `MalformedDtd`.
        // A limit is not malformed input -- the caller's remedy is to
        // raise a bound, not to fix the document -- so it is given the
        // kind that names the bound, here, where every error is made.
        let kind = match kind {
            ErrorKind::MalformedDtd(reason) => crate::dtd::error_kind(reason),
            other => other,
        };
        Self { kind, offset }
    }

    /// Recover the 1-based line and column for this error's offset.
    ///
    /// Counts in `char`s rather than bytes so the column is what a
    /// person looking at the file would count.
    #[must_use]
    pub fn line_column(&self, input: &str) -> (usize, usize) {
        // The offset comes from a byte-oriented scanner, so it can
        // land inside a multi-byte character -- an `IllegalCharacter`
        // offset routinely does. Slicing a `str` off a boundary
        // panics, and a diagnostic that aborts the process is worse
        // than the error it was trying to describe.
        let mut end = self.offset.min(input.len());
        while end > 0 && !input.is_char_boundary(end) {
            end -= 1;
        }
        let upto = &input[..end];
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
        write!(f, "at byte {}: {}", self.offset, self.kind)
    }
}

/// The message for each variant that reports a bound in [`Limits`]
/// being exceeded.
///
/// A table rather than arms of `Display`, because these grow with
/// `Limits` and each one names the limit a caller would raise.
///
/// [`Limits`]: crate::Limits
const LIMIT_MESSAGES: &[(ErrorKind, &str)] = &[
    (
        ErrorKind::DepthLimitExceeded,
        "elements nested past the depth limit",
    ),
    (
        ErrorKind::TooManyAttributes,
        "too many attributes on one element",
    ),
    (
        ErrorKind::AttributeTooLarge,
        "attribute value exceeds the size limit",
    ),
    (ErrorKind::NameTooLong, "name exceeds the length limit"),
    (ErrorKind::TooManyNodes, "document exceeds the node limit"),
    (ErrorKind::TextTooLong, "text node exceeds the length limit"),
    (
        ErrorKind::EntityLimitExceeded,
        "entity expansion exceeds the limit",
    ),
    (
        ErrorKind::ConstructTooLarge,
        "construct exceeds the streaming buffer limit",
    ),
    (ErrorKind::DtdTooLarge, "doctype exceeds the size limit"),
    (
        ErrorKind::DtdDepthLimitExceeded,
        "doctype exceeds the nesting limit",
    ),
    (
        ErrorKind::InputTooLarge,
        "source exceeds the input byte limit",
    ),
    (
        ErrorKind::DocumentTooLarge,
        "decoded document exceeds the byte limit",
    ),
];

impl ErrorKind {
    /// The message for a limit variant, or `None` for any other kind.
    fn limit_message(&self) -> Option<&'static str> {
        LIMIT_MESSAGES
            .iter()
            .find(|(kind, _)| kind == self)
            .map(|(_, message)| *message)
    }
}

/// The message without the offset.
///
/// A caller rendering a caret under the offending line already shows
/// the position, and repeating "at byte 41" beside the caret is noise.
impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(message) = self.limit_message() {
            return f.write_str(message);
        }
        match self {
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
            ErrorKind::ForbiddenEntityReference(n) => {
                write!(f, "`&{n};` may not be referenced here")
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
            ErrorKind::IllegalCdataEnd => {
                f.write_str("`]]>` must be written `]]&gt;` in content")
            }
            ErrorKind::ReservedNamespace => {
                f.write_str("reserved namespace prefix or URI")
            }
            ErrorKind::MalformedComment => {
                f.write_str("`--` is not allowed inside a comment")
            }
            ErrorKind::MalformedDeclaration => {
                f.write_str("malformed XML declaration")
            }
            ErrorKind::ReservedPiTarget => {
                f.write_str("`xml` is a reserved processing-instruction target")
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
            ErrorKind::Io(why) => write!(f, "reading the document: {why}"),
            ErrorKind::MalformedDtd(why) => {
                write!(f, "malformed doctype: {why}")
            }
            // A limit variant missing from `LIMIT_MESSAGES`. Rendering
            // the variant name is deliberate: the test that renders
            // every kind rejects a message that looks like `Debug`
            // output, so a new limit cannot ship without its message.
            _ => write!(f, "{self:?}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for Error {}

/// A parse result.
pub type Result<T> = core::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::string::{String, ToString};
    use alloc::vec::Vec;

    /// One value of every `ErrorKind` variant.
    ///
    /// Adding a variant without extending this list leaves its message
    /// unrendered by any test, which is how placeholder text reaches a
    /// release.
    fn every_kind() -> Vec<ErrorKind> {
        alloc::vec![
            ErrorKind::UnexpectedEof,
            ErrorKind::MismatchedEndTag {
                expected: String::from("a"),
                found: String::from("b"),
            },
            ErrorKind::UnexpectedEndTag(String::from("a")),
            ErrorKind::InvalidName,
            ErrorKind::UnquotedAttributeValue,
            ErrorKind::DuplicateAttribute(String::from("id")),
            ErrorKind::UnknownEntity(String::from("nope")),
            ErrorKind::ForbiddenEntityReference(String::from("ext")),
            ErrorKind::UnboundPrefix(String::from("p")),
            ErrorKind::TrailingContent,
            ErrorKind::NoRootElement,
            ErrorKind::Unterminated("comment"),
            ErrorKind::EntityLimitExceeded,
            ErrorKind::IllegalCdataEnd,
            ErrorKind::ReservedNamespace,
            ErrorKind::MalformedComment,
            ErrorKind::MalformedDeclaration,
            ErrorKind::ReservedPiTarget,
            ErrorKind::UnsupportedVersion,
            ErrorKind::IllegalCharacter('\u{7}'),
            ErrorKind::MalformedEncoding,
            ErrorKind::UnsupportedEncoding,
            ErrorKind::MalformedDtd("expected `>`"),
            ErrorKind::DepthLimitExceeded,
            ErrorKind::TooManyAttributes,
            ErrorKind::AttributeTooLarge,
            ErrorKind::NameTooLong,
            ErrorKind::TooManyNodes,
            ErrorKind::TextTooLong,
            ErrorKind::ConstructTooLarge,
            ErrorKind::DtdTooLarge,
        ]
    }

    #[test]
    fn every_error_renders_a_message_a_human_can_act_on() {
        for kind in every_kind() {
            let text = Error::new(kind.clone(), 7).to_string();
            assert!(
                text.starts_with("at byte 7: "),
                "{kind:?} lost its offset: {text:?}"
            );
            let body = &text["at byte 7: ".len()..];
            assert!(!body.is_empty(), "{kind:?} renders nothing");
            // A `Debug` fallback would echo the variant name verbatim.
            assert!(
                !body.starts_with(char::is_uppercase),
                "{kind:?} looks like a Debug fallback: {body:?}"
            );
        }
    }

    #[test]
    fn no_two_errors_render_the_same_message() {
        // Two distinct failures with one message is indistinguishable
        // to a caller reading a log.
        let mut seen: Vec<String> = every_kind()
            .into_iter()
            .map(|k| Error::new(k, 0).to_string())
            .collect();
        let before = seen.len();
        seen.sort();
        seen.dedup();
        assert_eq!(before, seen.len(), "duplicate error messages: {seen:?}");
    }

    #[test]
    fn messages_quote_the_names_they_are_about() {
        // The name is the useful part -- "mismatched end tag" alone
        // sends the reader back to the document to find which one.
        let text = Error::new(
            ErrorKind::MismatchedEndTag {
                expected: String::from("chapter"),
                found: String::from("section"),
            },
            0,
        )
        .to_string();
        assert!(
            text.contains("chapter") && text.contains("section"),
            "{text}"
        );

        for (kind, name) in [
            (
                ErrorKind::UnknownEntity(String::from("frobnicate")),
                "frobnicate",
            ),
            (
                ErrorKind::DuplicateAttribute(String::from("xml:id")),
                "xml:id",
            ),
            (ErrorKind::UnboundPrefix(String::from("svg")), "svg"),
            (ErrorKind::UnexpectedEndTag(String::from("br")), "br"),
        ] {
            let text = Error::new(kind, 0).to_string();
            assert!(text.contains(name), "{name:?} missing from {text:?}");
        }
    }

    #[test]
    fn an_illegal_character_is_named_by_code_point_not_printed_raw() {
        // Printing a control character into a terminal or log is how a
        // diagnostic becomes unreadable, or worse, an escape sequence.
        let text =
            Error::new(ErrorKind::IllegalCharacter('\u{7}'), 0).to_string();
        assert!(text.contains("U+0007"), "{text}");
        assert!(!text.contains('\u{7}'), "raw control character in message");
    }

    #[test]
    fn line_and_column_are_one_based_and_count_characters_not_bytes() {
        let input = "<a>\n  <b>héllo</b>\n</a>";
        assert_eq!(
            Error::new(ErrorKind::InvalidName, 0).line_column(input),
            (1, 1)
        );

        // Start of line 2.
        let at = input.find("  <b>").expect("line 2");
        assert_eq!(
            Error::new(ErrorKind::InvalidName, at).line_column(input),
            (2, 1)
        );

        // After the multi-byte `é`: column counts characters, so the
        // reported column matches what an editor shows.
        let at = input.find("llo").expect("past the accent");
        assert_eq!(
            Error::new(ErrorKind::InvalidName, at).line_column(input),
            (2, 8)
        );
    }

    #[test]
    fn an_offset_past_the_end_clamps_instead_of_panicking() {
        // Offsets can exceed the input after transcoding, and a
        // diagnostic that panics is worse than the error it describes.
        let input = "<a/>";
        let (line, col) =
            Error::new(ErrorKind::UnexpectedEof, 9_999).line_column(input);
        assert_eq!((line, col), (1, 5));
    }

    #[test]
    fn an_offset_inside_a_multibyte_character_does_not_panic() {
        // Slicing a `str` at a non-boundary panics; the offset comes
        // from a byte-oriented scanner, so it can land mid-character.
        let input = "<a>é</a>";
        let mid = input.find('é').expect("accent") + 1;
        let _ = Error::new(ErrorKind::IllegalCharacter('é'), mid)
            .line_column(input);
    }
}
