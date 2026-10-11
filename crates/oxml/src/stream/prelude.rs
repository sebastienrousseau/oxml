// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! What has to be known before the first byte can be judged.
//!
//! The XML declaration decides the version, and the version decides
//! what counts as a line ending and which characters are legal:
//! `\u{85}` and `\u{2028}` are line endings in 1.1 and ordinary
//! characters in 1.0. The declaration also names the encoding, and
//! the first bytes may carry a byte-order mark that overrules it. So
//! the bytes are held, undecoded, until the declaration has wholly
//! arrived or it is clear there is none, and only then does decoding
//! begin -- from the start, with the right decoder.
//!
//! Normalising the first kilobyte as 1.0 and redoing it once the
//! version was known turned `\r` NEL into two line feeds in a 1.1
//! document, because the pair was already split by the time the
//! answer was in hand.

use alloc::borrow::Cow;
use alloc::string::String;
use alloc::vec::Vec;

use crate::encoding::{Encoding, is_legal_encoding_name};
use crate::error::{Error, ErrorKind, Result};
use crate::parser::{
    Version, check_prolog_shape, declared_standalone, declared_version,
    normalize_line_endings,
};

const WS: [char; 4] = [' ', '\t', '\r', '\n'];

/// What the declaration and the first bytes settled.
#[derive(Debug, Clone, Copy)]
pub(super) struct Decision {
    pub(super) encoding: Encoding,
    /// Bytes of byte-order mark to skip.
    pub(super) bom: usize,
    pub(super) version: Version,
    pub(super) standalone: bool,
}

/// What the first bytes say about the encoding.
#[derive(Debug, Clone, Copy)]
struct Sniffed {
    /// From a byte-order mark, or from the shape of `<` in UTF-16.
    encoding: Option<Encoding>,
    bom: usize,
    /// Whether `encoding` came from a mark, which the declaration must
    /// then agree with.
    from_mark: bool,
}

/// Source bytes held until the declaration can be read.
#[derive(Debug, Default)]
pub(super) struct Prelude {
    bytes: Vec<u8>,
}

impl Prelude {
    pub(super) fn push(&mut self, bytes: &[u8]) {
        self.bytes.extend_from_slice(bytes);
    }

    pub(super) fn len(&self) -> usize {
        self.bytes.len()
    }

    pub(super) fn into_bytes(self) -> Vec<u8> {
        self.bytes
    }

    /// The decision, or `None` while too little has arrived to make
    /// it. `eof` means nothing more is coming, so it is made with what
    /// there is.
    ///
    /// The same checks [`crate::encoding::decode`] and the tree
    /// parser's `prepare` make, in the same order, so a document they
    /// refuse is refused here with the same error.
    pub(super) fn decide(&self, eof: bool) -> Result<Option<Decision>> {
        let Some(sniffed) = sniff(&self.bytes, eof) else {
            return Ok(None);
        };
        let body = &self.bytes[sniffed.bom..];
        let head = head_text(body, sniffed.encoding);
        let Some(decl) = declaration(&head, eof) else {
            return Ok(None);
        };
        let version = declared_version(decl)?;
        check_prolog_shape(&head)?;
        let encoding = choose_encoding(decl, sniffed)?;
        let standalone =
            declared_standalone(&normalize_line_endings(decl, version));
        Ok(Some(Decision {
            encoding,
            bom: sniffed.bom,
            version,
            standalone,
        }))
    }
}

/// The encoding the first bytes announce, if any, or `None` while
/// there are too few of them to say.
fn sniff(bytes: &[u8], eof: bool) -> Option<Sniffed> {
    if bytes.len() < 4 && !eof {
        return None;
    }
    if let Some(marked) = utf16_mark(bytes) {
        return Some(marked);
    }
    let bom = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else {
        0
    };
    Some(match utf16_shape(&bytes[bom..]) {
        Some(encoding) => Sniffed {
            encoding: Some(encoding),
            bom,
            from_mark: false,
        },
        None => Sniffed {
            encoding: (bom == 3).then_some(Encoding::Utf8),
            bom,
            from_mark: bom == 3,
        },
    })
}

/// A UTF-16 byte-order mark, which settles the encoding outright.
fn utf16_mark(bytes: &[u8]) -> Option<Sniffed> {
    let encoding = match bytes {
        [0xFE, 0xFF, ..] => Encoding::Utf16Be,
        [0xFF, 0xFE, ..] => Encoding::Utf16Le,
        _ => return None,
    };
    Some(Sniffed {
        encoding: Some(encoding),
        bom: 2,
        from_mark: true,
    })
}

/// UTF-16 without a mark is still detectable: an XML document must
/// begin with `<`, which in UTF-16 is a NUL-adjacent pair.
fn utf16_shape(rest: &[u8]) -> Option<Encoding> {
    match rest {
        [0x00, 0x3C, ..] => Some(Encoding::Utf16Be),
        [0x3C, 0x00, ..] => Some(Encoding::Utf16Le),
        _ => None,
    }
}

/// The held bytes as text, far enough to read a declaration.
///
/// A declaration is ASCII, so any 8-bit encoding reads it alike; what
/// cannot be read as UTF-8 is taken byte for byte, which loses
/// nothing and lets the `?>` be found past it. UTF-16 is decoded as
/// far as it decodes.
fn head_text(body: &[u8], encoding: Option<Encoding>) -> Cow<'_, str> {
    match encoding {
        Some(Encoding::Utf16Be) => Cow::Owned(utf16_head(body, true)),
        Some(Encoding::Utf16Le) => Cow::Owned(utf16_head(body, false)),
        _ => match core::str::from_utf8(body) {
            Ok(s) => Cow::Borrowed(s),
            Err(e) => {
                let valid = e.valid_up_to();
                let mut s = String::from(
                    core::str::from_utf8(&body[..valid]).unwrap_or_default(),
                );
                s.extend(body[valid..].iter().map(|&b| char::from(b)));
                Cow::Owned(s)
            }
        },
    }
}

fn utf16_head(body: &[u8], big_endian: bool) -> String {
    let units = body.chunks_exact(2).map(|pair| {
        let pair = [pair[0], pair[1]];
        if big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    });
    char::decode_utf16(units)
        .map_while(core::result::Result::ok)
        .collect()
}

/// The declaration at the start of `head`, `""` if there is none, or
/// `None` while it cannot yet be told which.
///
/// Only a `<?xml` followed by whitespace is a declaration, as the
/// tree parser's `xml_declaration` has it: `<?xml-stylesheet?>` at
/// offset 0 is a processing instruction.
fn declaration(head: &str, eof: bool) -> Option<&str> {
    let trimmed = head.trim_start_matches(WS);
    if trimmed.len() < 6 && !eof {
        return None;
    }
    let is_decl = head
        .strip_prefix("<?xml")
        .is_some_and(|rest| rest.starts_with(WS));
    if !is_decl {
        return Some("");
    }
    match head.find("?>") {
        Some(end) => Some(&head[..end + 2]),
        None if eof => Some(head),
        None => None,
    }
}

/// Which decoder to use, from the mark and the declaration together.
fn choose_encoding(decl: &str, sniffed: Sniffed) -> Result<Encoding> {
    // UTF-16 told from the shape of its first bytes rather than from a
    // mark: the declaration is not consulted, as `decode` does not.
    if let (Some(encoding), false) = (sniffed.encoding, sniffed.from_mark) {
        return Ok(encoding);
    }
    let declared = declared_encoding(decl)?;
    match (sniffed.encoding, declared) {
        // A mark and a declaration that disagree make the document
        // malformed: the mark wins silently in no conforming parser.
        (Some(marked), Some(declared)) if !agree(marked, declared) => {
            Err(Error::new(ErrorKind::MalformedEncoding, 0))
        }
        (Some(marked), _) => Ok(marked),
        (None, declared) => Ok(declared.unwrap_or(Encoding::Utf8)),
    }
}

/// Whether a declared encoding is in the family a mark announced.
/// `UTF-16` and `UTF-16LE` both agree with a little-endian mark: the
/// mark settles a byte order the name leaves open.
fn agree(marked: Encoding, declared: Encoding) -> bool {
    match marked {
        Encoding::Utf8 => declared == Encoding::Utf8,
        Encoding::Utf16Be | Encoding::Utf16Le => {
            matches!(declared, Encoding::Utf16Be | Encoding::Utf16Le)
        }
        Encoding::Latin1 => false,
    }
}

/// The `encoding` pseudo-attribute of a declaration, as
/// `encoding::decode` reads it: an illegal name is malformed, a legal
/// name this crate cannot decode is unsupported, and either is
/// reported at the pseudo-attribute.
fn declared_encoding(decl: &str) -> Result<Option<Encoding>> {
    let Some(at) = decl.find("encoding") else {
        return Ok(None);
    };
    let rest = decl[at + "encoding".len()..].trim_start_matches(WS);
    let Some(rest) = rest.strip_prefix('=') else {
        return Ok(None);
    };
    let rest = rest.trim_start_matches(WS);
    let Some(quote) = rest.chars().next().filter(|c| matches!(c, '"' | '\''))
    else {
        return Ok(None);
    };
    let body = &rest[1..];
    let Some(close) = body.find(quote) else {
        return Ok(None);
    };
    let name = &body[..close];
    if !is_legal_encoding_name(name) {
        return Err(Error::new(ErrorKind::MalformedEncoding, at));
    }
    Encoding::from_name(name)
        .map(Some)
        .ok_or_else(|| Error::new(ErrorKind::UnsupportedEncoding, at))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decide(bytes: &[u8], eof: bool) -> Result<Option<Decision>> {
        let mut p = Prelude::default();
        p.push(bytes);
        p.decide(eof)
    }

    #[test]
    fn the_decision_waits_for_the_whole_declaration() {
        assert!(
            decide(b"<?xml version=\"1.1\"", false)
                .expect("fine")
                .is_none()
        );
        let d = decide(b"<?xml version=\"1.1\"?><a/>", false)
            .expect("fine")
            .expect("decided");
        assert_eq!(d.version, Version::V11);
        assert_eq!(d.encoding, Encoding::Utf8);
        assert!(decide(b"<a", false).expect("fine").is_none(), "six bytes");
        assert!(decide(b"<a/>  ", false).expect("fine").is_some());
        assert!(decide(b"<a", true).expect("fine").is_some(), "at eof");
    }

    #[test]
    fn marks_and_declarations_decide_the_encoding_as_decode_does() {
        let cases: [(&[u8], Encoding, usize); 5] = [
            (b"\xEF\xBB\xBF<a/>", Encoding::Utf8, 3),
            (b"\xFF\xFE<\x00a\x00/\x00>\x00", Encoding::Utf16Le, 2),
            (b"\xFE\xFF\x00<\x00a\x00/\x00>", Encoding::Utf16Be, 2),
            (b"<\x00a\x00/\x00>\x00", Encoding::Utf16Le, 0),
            (
                b"<?xml version='1.0' encoding='iso-8859-1'?><a/>",
                Encoding::Latin1,
                0,
            ),
        ];
        for (bytes, encoding, bom) in cases {
            let d = decide(bytes, true).expect("fine").expect("decided");
            assert_eq!((d.encoding, d.bom), (encoding, bom), "{bytes:?}");
        }
    }

    #[test]
    fn errors_are_the_ones_decode_and_prepare_report() {
        let cases: [(&[u8], ErrorKind, usize); 4] = [
            (
                b" <?xml version='1.0'?><a/>",
                ErrorKind::MalformedDeclaration,
                0,
            ),
            (
                b"<?xml version='3.0'?><a/>",
                ErrorKind::UnsupportedVersion,
                0,
            ),
            (
                b"<?xml version='1.0' encoding='EBCDIC-CP-US'?><a/>",
                ErrorKind::UnsupportedEncoding,
                20,
            ),
            (
                b"\xEF\xBB\xBF<?xml version='1.0' encoding='iso-8859-1'?><a/>",
                ErrorKind::MalformedEncoding,
                0,
            ),
        ];
        for (bytes, kind, offset) in cases {
            let e = decide(bytes, true).expect_err("refused");
            assert_eq!((&e.kind, e.offset), (&kind, offset), "{bytes:?}");
            let from_decode = crate::encoding::decode(bytes)
                .map_err(|e| (e.kind, e.offset))
                .and_then(|text| {
                    crate::parser::prepare(&text, crate::Limits::default())
                        .map(|_| ())
                        .map_err(|e| (e.kind, e.offset))
                });
            assert_eq!(from_decode, Err((kind, offset)), "{bytes:?}");
        }
    }
}
