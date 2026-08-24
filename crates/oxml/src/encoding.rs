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
    // A BOM and a declaration that disagree make the document
    // malformed. Letting the BOM win silently produced a tree from a
    // document no conforming parser accepts -- and the disagreement is
    // usually a sign that something upstream re-encoded the bytes
    // without rewriting the declaration, which is worth surfacing.
    if let Some(rest) = bytes.strip_prefix(&[0xFE, 0xFF]) {
        let text = utf16(rest, true)?;
        check_bom_agrees(&text, Encoding::Utf16Be)?;
        return Ok(Cow::Owned(text));
    }
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        let text = utf16(rest, false)?;
        check_bom_agrees(&text, Encoding::Utf16Le)?;
        return Ok(Cow::Owned(text));
    }
    // A UTF-8 BOM is permitted and is not part of the document.
    if let Some(rest) = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]) {
        if let Ok(text) = core::str::from_utf8(rest) {
            check_bom_agrees(text, Encoding::Utf8)?;
        }
    }
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

/// Reject a declaration that contradicts the byte-order mark.
///
/// A UTF-16 BOM with `encoding="utf-8"`, or a UTF-8 BOM with
/// `encoding="iso-8859-1"`, cannot both be true. The family has to
/// match: `UTF-16` and `UTF-16LE` agree with a little-endian mark,
/// because the mark settles a byte order the name leaves open.
fn check_bom_agrees(text: &str, from_bom: Encoding) -> Result<()> {
    let Ok(Some(declared)) = declared_encoding(text.as_bytes()) else {
        return Ok(());
    };
    let compatible = match (from_bom, declared) {
        (Encoding::Utf8, Encoding::Utf8) => true,
        (Encoding::Utf16Be | Encoding::Utf16Le, d) => {
            matches!(d, Encoding::Utf16Be | Encoding::Utf16Le)
        }
        _ => false,
    };
    if compatible {
        Ok(())
    } else {
        Err(Error::new(ErrorKind::MalformedEncoding, 0))
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
///
/// The surrogate arithmetic is delegated to `core`, so the only failure
/// this function decides for itself is a byte count that cannot be a
/// whole number of code units. Offsets point at the first byte of the
/// offending unit.
fn utf16(bytes: &[u8], big_endian: bool) -> Result<String> {
    if bytes.len() % 2 != 0 {
        return Err(Error::new(ErrorKind::MalformedEncoding, bytes.len() - 1));
    }
    #[allow(clippy::chunks_exact_to_as_chunks)]
    // `as_chunks` is unstable on MSRV
    let units = bytes.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    });

    let mut out = String::with_capacity(bytes.len() / 2);
    let mut offset = 0;
    for unit in char::decode_utf16(units) {
        // An unpaired surrogate is not a character in any encoding, and
        // silently substituting U+FFFD would hide a corrupt document.
        let c =
            unit.map_err(|_| Error::new(ErrorKind::MalformedEncoding, offset))?;
        offset += c.len_utf16() * 2;
        out.push(c);
    }
    Ok(out)
}

/// Decode ISO-8859-1, where every byte is the code point of the same
/// value. Cannot fail.
fn latin1(bytes: &[u8]) -> String {
    bytes.iter().map(|&b| char::from(b)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode `text` as UTF-16 with a byte-order mark.
    fn utf16_bom(text: &str, big_endian: bool) -> alloc::vec::Vec<u8> {
        let mut out = if big_endian {
            alloc::vec![0xFE, 0xFF]
        } else {
            alloc::vec![0xFF, 0xFE]
        };
        for unit in text.encode_utf16() {
            let b = if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            };
            out.extend_from_slice(&b);
        }
        out
    }

    #[test]
    fn utf8_is_borrowed_not_copied() {
        // The zero-copy path is the whole reason UTF-8 is special-cased;
        // if this starts allocating, the fast path is gone.
        let bytes = b"<a>hello</a>";
        match decode(bytes).expect("valid utf-8") {
            Cow::Borrowed(s) => assert_eq!(s, "<a>hello</a>"),
            Cow::Owned(_) => panic!("UTF-8 input must not be transcoded"),
        }
    }

    #[test]
    fn a_utf8_bom_is_stripped_and_is_not_content() {
        let mut bytes = alloc::vec![0xEF, 0xBB, 0xBF];
        bytes.extend_from_slice(b"<a/>");
        assert_eq!(decode(&bytes).expect("valid"), "<a/>");
    }

    #[test]
    fn utf16_decodes_in_both_byte_orders() {
        for big_endian in [true, false] {
            let bytes = utf16_bom("<a>héllo</a>", big_endian);
            assert_eq!(
                decode(&bytes).expect("valid utf-16"),
                "<a>héllo</a>",
                "big_endian={big_endian}"
            );
        }
    }

    #[test]
    fn utf16_surrogate_pairs_decode_to_one_character() {
        // Non-BMP characters arrive as a pair; neither half is a
        // character on its own.
        for big_endian in [true, false] {
            let bytes = utf16_bom("<a>😀</a>", big_endian);
            assert_eq!(decode(&bytes).expect("valid"), "<a>😀</a>");
        }
    }

    #[test]
    fn utf16_is_detected_without_a_bom() {
        // An XML document must begin with `<`, which in UTF-16 is a
        // NUL-adjacent pair — enough to tell the byte order.
        let be: alloc::vec::Vec<u8> =
            "<a/>".encode_utf16().flat_map(u16::to_be_bytes).collect();
        let le: alloc::vec::Vec<u8> =
            "<a/>".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(decode(&be).expect("be"), "<a/>");
        assert_eq!(decode(&le).expect("le"), "<a/>");
    }

    #[test]
    fn malformed_utf16_is_rejected_rather_than_replaced() {
        // An odd byte count cannot be UTF-16 at all.
        let odd = alloc::vec![0xFF, 0xFE, 0x3C, 0x00, 0x41];
        assert!(decode(&odd).is_err(), "odd length");

        // A lone high surrogate, and a lone low one.
        let lone_high = alloc::vec![0xFF, 0xFE, 0x00, 0xD8, 0x41, 0x00];
        assert!(decode(&lone_high).is_err(), "lone high surrogate");
        let lone_low = alloc::vec![0xFF, 0xFE, 0x00, 0xDC];
        assert!(decode(&lone_low).is_err(), "lone low surrogate");
    }

    #[test]
    fn latin1_maps_every_byte_to_its_code_point() {
        let mut bytes =
            b"<?xml version='1.0' encoding='iso-8859-1'?><a>".to_vec();
        bytes.push(0xE9); // é in Latin-1
        bytes.extend_from_slice(b"</a>");
        let text = decode(&bytes).expect("valid latin-1");
        assert!(text.ends_with("<a>é</a>"), "{text}");
    }

    #[test]
    fn invalid_utf8_claiming_to_be_utf8_is_rejected() {
        // A lone continuation byte is not valid UTF-8, and the document
        // says it is UTF-8, so this is malformed rather than merely
        // undecodable.
        let bytes = alloc::vec![b'<', b'a', b'>', 0x80, b'<', b'/', b'a', b'>'];
        let err = decode(&bytes).expect_err("not valid utf-8");
        assert_eq!(err.kind, ErrorKind::MalformedEncoding);
    }

    #[test]
    fn an_illegal_encoding_name_is_a_wellformedness_error() {
        // `EncName ::= [A-Za-z] ([A-Za-z0-9._] | '-')*`. These are not
        // "an encoding we lack" — they are malformed documents.
        for bad in ["UTF~8", "utf:8", "-UTF-8", ".UTF-8", "8-UTF", "a/b", ""] {
            assert!(!is_legal_encoding_name(bad), "{bad} should be illegal");
        }
        for good in ["UTF-8", "utf8", "ISO-8859-1", "Shift_JIS", "x.y-z9"] {
            assert!(is_legal_encoding_name(good), "{good} should be legal");
        }
    }

    #[test]
    fn a_malformed_name_and_an_unsupported_one_are_different_errors() {
        // The distinction matters: one is the document's fault, the
        // other is ours, and a conformance runner must score them
        // differently.
        let malformed =
            b"<?xml version='1.0' encoding='UTF~8'?><a/>".as_slice();
        assert_eq!(
            decode(malformed).expect_err("illegal name").kind,
            ErrorKind::MalformedEncoding
        );

        let unsupported =
            b"<?xml version='1.0' encoding='Shift_JIS'?><a/>".as_slice();
        assert_eq!(
            decode(unsupported).expect_err("not supported").kind,
            ErrorKind::UnsupportedEncoding
        );
    }

    #[test]
    fn encoding_names_resolve_case_insensitively() {
        for (name, want) in [
            ("UTF-8", Encoding::Utf8),
            ("utf-8", Encoding::Utf8),
            ("UtF8", Encoding::Utf8),
            ("us-ascii", Encoding::Utf8),
            ("UTF-16BE", Encoding::Utf16Be),
            ("utf-16le", Encoding::Utf16Le),
            ("ISO-8859-1", Encoding::Latin1),
            ("latin1", Encoding::Latin1),
        ] {
            assert_eq!(Encoding::from_name(name), Some(want), "{name}");
        }
        assert_eq!(Encoding::from_name("Shift_JIS"), None);
    }

    #[test]
    fn a_bom_that_contradicts_the_declaration_is_an_error() {
        // The two cannot both be true, and letting the mark win
        // silently produced a tree from a document no conforming
        // parser accepts. In practice the disagreement means something
        // upstream re-encoded the bytes without rewriting the
        // declaration, which is worth surfacing rather than papering
        // over.
        let mut bytes = alloc::vec![0xFF, 0xFE];
        for unit in "<?xml version='1.0' encoding='UTF-8'?><a/>".encode_utf16()
        {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert_eq!(
            decode(&bytes)
                .expect_err("UTF-16 mark, UTF-8 declared")
                .kind,
            ErrorKind::MalformedEncoding
        );

        let mut utf8_bom = alloc::vec![0xEF, 0xBB, 0xBF];
        utf8_bom.extend_from_slice(
            b"<?xml version='1.0' encoding='iso-8859-1'?><a/>",
        );
        assert_eq!(
            decode(&utf8_bom)
                .expect_err("UTF-8 mark, Latin-1 declared")
                .kind,
            ErrorKind::MalformedEncoding
        );
    }

    #[test]
    fn a_bom_that_agrees_with_the_declaration_is_fine() {
        // The families have to match, not the exact name: a
        // little-endian mark agrees with `UTF-16`, because the mark
        // settles a byte order the name leaves open.
        let mut bytes = alloc::vec![0xFF, 0xFE];
        for unit in "<?xml version='1.0' encoding='UTF-16'?><a/>".encode_utf16()
        {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        assert!(decode(&bytes).is_ok(), "UTF-16 mark, UTF-16 declared");

        let mut utf8_bom = alloc::vec![0xEF, 0xBB, 0xBF];
        utf8_bom
            .extend_from_slice(b"<?xml version='1.0' encoding='UTF-8'?><a/>");
        assert!(decode(&utf8_bom).is_ok(), "UTF-8 mark, UTF-8 declared");
    }

    #[test]
    fn a_document_with_no_declaration_defaults_to_utf8() {
        assert!(matches!(
            decode(b"<a>plain</a>").expect("valid"),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn a_declaration_that_does_not_parse_leaves_the_default_in_place() {
        // Every one of these is a declaration the encoding scanner
        // cannot make sense of. None of them is an *encoding* error:
        // the document simply hasn't named an encoding, so UTF-8
        // applies and any real problem surfaces in the parser instead.
        for decl in [
            "<?xml version='1.0'",     // never terminated
            "<?xml version='1.0'?>",   // no encoding pseudo-attribute
            "<?xml encoding?>",        // no `=`
            "<?xml encoding=?>",       // no value at all
            "<?xml encoding=UTF-8?>",  // unquoted value
            "<?xml encoding='UTF-8?>", // never closed
            "<?xmlno-space?>",         // not a declaration
        ] {
            let bytes = alloc::format!("{decl}<a/>");
            assert!(
                matches!(decode(bytes.as_bytes()), Ok(Cow::Borrowed(_))),
                "{decl} should fall back to UTF-8"
            );
        }
    }

    #[test]
    fn whitespace_is_allowed_around_the_equals_sign() {
        let bytes = b"<?xml version='1.0' encoding \t=\r\n 'iso-8859-1'?><a/>";
        assert!(
            matches!(decode(bytes.as_slice()), Ok(Cow::Owned(_))),
            "spaced-out declaration should still select Latin-1"
        );
    }

    #[test]
    fn a_truncated_surrogate_pair_at_end_of_input_is_rejected() {
        // The high half arrives and the buffer simply stops. This is a
        // different code path from a high half followed by a non-low
        // unit, and truncation is the likelier real-world failure.
        let bytes = alloc::vec![0xFF, 0xFE, 0x3D, 0xD8];
        assert!(decode(&bytes).is_err(), "truncated pair");
    }

    #[test]
    fn a_declaration_may_claim_utf16_while_the_bytes_are_not() {
        // An ASCII declaration announcing UTF-16 contradicts itself:
        // were the document really UTF-16, the declaration would be
        // too. The bytes are decoded as declared, and fail.
        let bytes = b"<?xml version='1.0' encoding='UTF-16'?><a/>";
        assert_eq!(
            decode(bytes.as_slice()).expect_err("odd length").kind,
            ErrorKind::MalformedEncoding
        );
        // Even byte count, so it decodes -- into mojibake, not `<a/>`.
        for label in ["UTF-16LE", "UTF-16BE"] {
            let src =
                alloc::format!("<?xml version='1.0' encoding='{label}'?><a/> ");
            let text = decode(src.as_bytes()).expect("even length");
            assert!(!text.contains("<a/>"), "{label}: {text}");
        }
    }
}
