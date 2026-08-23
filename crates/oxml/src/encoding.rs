// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Turning bytes into the `&str` the parser wants.
//!
//! [`crate::parse`] takes `&str` and borrows from it. That is the fast
//! path and stays exactly as it was. This module adds a byte-oriented
//! entry point for the cases `&str` cannot express: a document in
//! UTF-16, a document in Latin-1, or a document claiming to be UTF-8
//! that is not.
//!
//! # Why this is not `encoding_rs`
//!
//! Only three encodings appear in practice for XML — UTF-8, UTF-16 in
//! either byte order, and Latin-1 — and each is a few lines. Pulling in
//! a 300 KB table-driven transcoder for that would cost more than it
//! buys, and this crate has no dependencies by design. A caller needing
//! Shift-JIS can decode themselves and call [`crate::parse`].
//!
//! # Interaction with borrowing
//!
//! Decoding UTF-16 necessarily allocates: the output is a different
//! byte sequence from the input, so nothing can borrow from it. UTF-8
//! input is passed through untouched and keeps the zero-copy path.

use alloc::borrow::Cow;
use alloc::string::String;

use crate::error::{Error, ErrorKind, Result};

/// A character encoding this crate can decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Encoding {
    /// UTF-8, the fast path — no transcoding.
    Utf8,
    /// UTF-16, big-endian.
    Utf16Be,
    /// UTF-16, little-endian.
    Utf16Le,
    /// ISO-8859-1, also called Latin-1.
    Latin1,
}

impl Encoding {
    /// Resolve a declared encoding name, case-insensitively.
    ///
    /// Returns `None` for a name that is well-formed but names an
    /// encoding this crate cannot decode — a different condition from a
    /// name that is not a legal `EncName` at all, which is a
    /// well-formedness error and is rejected before this is reached.
    #[must_use]
    pub fn from_name(name: &str) -> Option<Self> {
        // `eq_ignore_ascii_case` rather than allocating a lowercase
        // copy: encoding names are ASCII by definition of `EncName`.
        for (label, enc) in [
            ("utf-8", Self::Utf8),
            ("utf8", Self::Utf8),
            ("us-ascii", Self::Utf8),
            ("ascii", Self::Utf8),
            ("utf-16", Self::Utf16Le),
            ("utf-16le", Self::Utf16Le),
            ("utf-16be", Self::Utf16Be),
            ("iso-8859-1", Self::Latin1),
            ("latin1", Self::Latin1),
        ] {
            if name.eq_ignore_ascii_case(label) {
                return Some(enc);
            }
        }
        None
    }
}

/// Whether `name` is a legal `EncName` per XML 1.0 production 81.
///
/// `EncName ::= [A-Za-z] ([A-Za-z0-9._] | '-')*`
///
/// This is a well-formedness constraint, so a document declaring
/// `encoding="UTF~8"` is not merely using an encoding we lack — it is
/// malformed, and must be rejected by every parser.
#[must_use]
pub fn is_legal_encoding_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(c) if c.is_ascii_alphabetic() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// Decode `bytes` into text the parser can read.
///
/// The encoding is determined in the order the specification requires:
/// a byte-order mark wins, then the `encoding` pseudo-attribute of the
/// XML declaration, then UTF-8 by default.
///
/// # Errors
///
/// Returns [`Error`] if the declared encoding name is not a legal
/// `EncName`, if it names an encoding this crate cannot decode, or if
/// the bytes are not valid in the encoding they claim.
pub fn decode(bytes: &[u8]) -> Result<Cow<'_, str>> {
    // A BOM is authoritative and overrides any declaration.
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        return Ok(Cow::Owned(utf16(rest, true)?));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return Ok(Cow::Owned(utf16(rest, false)?));
    }
    // A UTF-8 BOM is permitted and is not part of the document.
    let bytes = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);

    // UTF-16 without a BOM is still detectable: an XML document must
    // begin with `<`, which in UTF-16 is a NUL-adjacent pair.
    if bytes.starts_with(&[0x00, 0x3C]) {
        return Ok(Cow::Owned(utf16(bytes, true)?));
    }
    if bytes.starts_with(&[0x3C, 0x00]) {
        return Ok(Cow::Owned(utf16(bytes, false)?));
    }

    match declared_encoding(bytes)? {
        Some(Encoding::Latin1) => Ok(Cow::Owned(latin1(bytes))),
        Some(Encoding::Utf16Be) => Ok(Cow::Owned(utf16(bytes, true)?)),
        Some(Encoding::Utf16Le) => Ok(Cow::Owned(utf16(bytes, false)?)),
        // UTF-8, declared or defaulted: no transcoding, so the parser
        // can still borrow from the caller's buffer.
        Some(Encoding::Utf8) | None => {
            core::str::from_utf8(bytes).map(Cow::Borrowed).map_err(|e| {
                Error::new(ErrorKind::MalformedEncoding, e.valid_up_to())
            })
        }
    }
}

/// The `encoding` pseudo-attribute of the XML declaration, if any.
fn declared_encoding(bytes: &[u8]) -> Result<Option<Encoding>> {
    if !bytes.starts_with(b"<?xml") {
        return Ok(None);
    }
    let Some(end) = find(bytes, b"?>") else {
        return Ok(None);
    };
    let decl = &bytes[..end];
    let Some(at) = find(decl, b"encoding") else {
        return Ok(None);
    };
    let rest = &decl[at + b"encoding".len()..];
    let rest = trim_start(rest);
    let Some(rest) = rest.strip_prefix(b"=") else {
        return Ok(None);
    };
    let rest = trim_start(rest);
    let (Some(quote), Some(body)) = (rest.first().copied(), rest.get(1..))
    else {
        return Ok(None);
    };
    if quote != b'"' && quote != b'\'' {
        return Ok(None);
    }
    let Some(close) = body.iter().position(|&b| b == quote) else {
        return Ok(None);
    };
    let name = core::str::from_utf8(&body[..close])
        .map_err(|_| Error::new(ErrorKind::MalformedEncoding, at))?;

    if !is_legal_encoding_name(name) {
        return Err(Error::new(ErrorKind::MalformedEncoding, at));
    }
    Encoding::from_name(name)
        .map(Some)
        .ok_or_else(|| Error::new(ErrorKind::UnsupportedEncoding, at))
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

fn trim_start(mut b: &[u8]) -> &[u8] {
    while matches!(b.first(), Some(b' ' | b'\t' | b'\r' | b'\n')) {
        b = &b[1..];
    }
    b
}

/// Decode UTF-16, handling surrogate pairs.
fn utf16(bytes: &[u8], big_endian: bool) -> Result<String> {
    if bytes.len() % 2 != 0 {
        return Err(Error::new(ErrorKind::MalformedEncoding, bytes.len() - 1));
    }
    let mut out = String::with_capacity(bytes.len() / 2);
    let mut i = 0;
    while i + 1 < bytes.len() {
        let unit = if big_endian {
            u16::from_be_bytes([bytes[i], bytes[i + 1]])
        } else {
            u16::from_le_bytes([bytes[i], bytes[i + 1]])
        };
        i += 2;
        // A non-BMP character is a surrogate *pair*; neither half is a
        // character on its own.
        if (0xD800..0xDC00).contains(&unit) {
            if i + 1 >= bytes.len() {
                return Err(Error::new(ErrorKind::MalformedEncoding, i));
            }
            let low = if big_endian {
                u16::from_be_bytes([bytes[i], bytes[i + 1]])
            } else {
                u16::from_le_bytes([bytes[i], bytes[i + 1]])
            };
            i += 2;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(Error::new(ErrorKind::MalformedEncoding, i));
            }
            let cp = 0x1_0000
                + ((u32::from(unit) - 0xD800) << 10)
                + (u32::from(low) - 0xDC00);
            out.push(
                char::from_u32(cp).ok_or_else(|| {
                    Error::new(ErrorKind::MalformedEncoding, i)
                })?,
            );
        } else if (0xDC00..0xE000).contains(&unit) {
            // An unpaired low surrogate.
            return Err(Error::new(ErrorKind::MalformedEncoding, i));
        } else {
            out.push(
                char::from_u32(u32::from(unit)).ok_or_else(|| {
                    Error::new(ErrorKind::MalformedEncoding, i)
                })?,
            );
        }
    }
    Ok(out)
}

/// Decode ISO-8859-1, where every byte is the code point of the same
/// value. Cannot fail.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}
