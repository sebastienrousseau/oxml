// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Escaping, character references, and the I/O adapter.

use core::fmt::Write;
#[cfg(feature = "std")]
use std::io;

/// Captures the first I/O error behind a `fmt::Write` face.
///
/// `core::fmt::Error` carries nothing, so the error is kept here and
/// handed back by [`Document::write_xml_to`](crate::Document::write_xml_to) once writing stops.
#[cfg(feature = "std")]
pub(super) struct IoSink<W> {
    pub(super) inner: W,
    pub(super) error: Option<io::Error>,
}

#[cfg(feature = "std")]
impl<W: io::Write> Write for IoSink<W> {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        self.inner.write_all(s.as_bytes()).map_err(|e| {
            self.error = Some(e);
            core::fmt::Error
        })
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
pub(super) fn write_text<W: Write>(
    text: &str,
    out: &mut W,
) -> core::fmt::Result {
    write_escaped(text, out, escape_text)
}

/// The markup for a character that character data cannot hold
/// literally, or `None` for one it can.
const fn escape_text(c: char) -> Option<&'static str> {
    match c {
        '&' => Some("&amp;"),
        '<' => Some("&lt;"),
        '>' => Some("&gt;"),
        '\r' => Some("&#13;"),
        _ => None,
    }
}

/// Write `text` with `escape` applied, and the characters it leaves
/// alone written literally or as references.
fn write_escaped<W: Write>(
    text: &str,
    out: &mut W,
    escape: fn(char) -> Option<&'static str>,
) -> core::fmt::Result {
    for c in text.chars() {
        match escape(c) {
            Some(markup) => out.write_str(markup)?,
            None if needs_reference(c) => write_reference(c, out)?,
            None => out.write_char(c)?,
        }
    }
    Ok(())
}

/// A C0 control character other than tab, newline and carriage
/// return: what XML 1.0 cannot express at all.
pub(super) const fn is_c0_control(c: char) -> bool {
    matches!(c, '\u{1}'..='\u{8}' | '\u{b}' | '\u{c}' | '\u{e}'..='\u{1f}')
}

/// Whether a character is written as a reference.
///
/// The C0 controls must be: XML 1.1 permits them only in that form.
/// The C1 range `U+007F..U+009F` is legal literally in 1.0 but
/// restricted in 1.1, and `U+0085` is a line ending there, so it is
/// written as a reference too -- which both versions accept. This is
/// deliberately wider than [`is_c0_control`], which decides the
/// version declaration.
///
/// `U+2028` is the other 1.1 line ending. Written literally into a
/// document that needs 1.1, it read back as a newline in character
/// data and as a space in an attribute value.
pub(super) const fn needs_reference(c: char) -> bool {
    is_c0_control(c) || matches!(c, '\u{7f}'..='\u{9f}' | '\u{2028}')
}

/// Write one character as a numeric reference.
pub(super) fn write_reference<W: Write>(
    c: char,
    out: &mut W,
) -> core::fmt::Result {
    write!(out, "&#{};", c as u32)
}

/// Escape an attribute value.
///
/// Tab, newline and carriage return are written as character
/// references rather than literally. Attribute-value normalisation
/// turns a literal one into a space when the document is read back, so
/// writing them literally would lose them -- silently, and only for
/// values that happened to contain whitespace.
pub(super) fn write_attribute_value<W: Write>(
    value: &str,
    out: &mut W,
) -> core::fmt::Result {
    write_escaped(value, out, escape_attribute_value)
}

/// The markup for a character an attribute value cannot hold
/// literally: what character data cannot, plus the quote and the
/// whitespace that normalisation would rewrite.
const fn escape_attribute_value(c: char) -> Option<&'static str> {
    match c {
        '"' => Some("&quot;"),
        '\t' => Some("&#9;"),
        '\n' => Some("&#10;"),
        _ => escape_text(c),
    }
}
