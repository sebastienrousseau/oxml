// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Bytes in, normalised text out, a chunk at a time.
//!
//! Three things have to survive a chunk boundary that can fall
//! anywhere:
//!
//! - a multi-byte character or a UTF-16 surrogate pair split across
//!   two chunks, held back until the rest arrives;
//! - a `\r\n` pair (or `\r` NEL, in XML 1.1) split across two chunks,
//!   held as a pending `\r` so it normalises to one `\n` rather than
//!   two;
//! - the offset, which is the document's and not this chunk's.
//!
//! Each of those, got wrong, produces a document that reads correctly
//! at one buffer size and not another -- the worst kind of bug to
//! find from a report.

use alloc::string::String;

use crate::encoding::Encoding;
use crate::error::{Error, ErrorKind, Result};
use crate::parser::{Version, check_characters};

/// The decoding state between chunks.
#[derive(Debug)]
pub(super) struct Decoder {
    encoding: Encoding,
    version: Version,
    /// The bytes of a UTF-8 sequence whose rest has not arrived.
    partial: [u8; 4],
    partial_len: usize,
    /// The first byte of a UTF-16 code unit split across chunks.
    odd: Option<u8>,
    /// A UTF-16 high surrogate whose partner has not arrived.
    high: Option<u16>,
    /// A trailing `\r` held back: its partner may be in the next
    /// chunk.
    pending_cr: bool,
    /// Source bytes taken so far, the byte-order mark excluded and
    /// those held back included. Decoding errors are reported against
    /// it, as [`crate::encoding::decode`] reports them.
    source_pos: usize,
    /// UTF-8 bytes decoded before XML line-ending normalization.
    decoded_bytes: usize,
    max_document_bytes: Option<usize>,
    /// Transcoded text on its way to the buffer, reused across chunks.
    scratch: String,
}

/// How many bytes a UTF-8 sequence beginning with `lead` has.
///
/// Only called for a lead byte that `from_utf8` has already judged
/// the start of an incomplete, otherwise valid sequence.
const fn utf8_width(lead: u8) -> usize {
    if lead >= 0xF0 {
        4
    } else if lead >= 0xE0 {
        3
    } else {
        2
    }
}

const fn is_high_surrogate(u: u16) -> bool {
    matches!(u, 0xD800..=0xDBFF)
}

const fn is_low_surrogate(u: u16) -> bool {
    matches!(u, 0xDC00..=0xDFFF)
}

/// A high surrogate and the unit after it: four bytes if they pair.
fn surrogate_pair(high: u16, low: u16, out: &mut String) -> Option<usize> {
    if !is_low_surrogate(low) {
        return None;
    }
    let c = char::decode_utf16([high, low]).next()?.ok()?;
    out.push(c);
    Some(4)
}

/// A unit outside the surrogate range: two bytes, one character.
fn single_unit(unit: u16, out: &mut String) -> Option<usize> {
    if is_low_surrogate(unit) {
        return None;
    }
    out.push(char::from_u32(u32::from(unit))?);
    Some(2)
}

/// The valid UTF-8 at the front of `bytes` and the incomplete
/// sequence at its end, or where an invalid sequence begins.
fn valid_prefix(bytes: &[u8]) -> core::result::Result<(&str, &[u8]), usize> {
    match core::str::from_utf8(bytes) {
        Ok(s) => Ok((s, &[])),
        Err(e) if e.error_len().is_some() => Err(e.valid_up_to()),
        Err(e) => {
            // Incomplete rather than invalid: the rest is coming.
            let valid = e.valid_up_to();
            let whole =
                core::str::from_utf8(&bytes[..valid]).unwrap_or_default();
            Ok((whole, &bytes[valid..]))
        }
    }
}

/// UTF-16 code units over a chunk, with one byte carried in from the
/// previous chunk and at most one carried out to the next.
struct Units<'b> {
    odd: Option<u8>,
    rest: &'b [u8],
    big_endian: bool,
}

impl Units<'_> {
    fn unit(&self, pair: [u8; 2]) -> u16 {
        if self.big_endian {
            u16::from_be_bytes(pair)
        } else {
            u16::from_le_bytes(pair)
        }
    }
}

impl Units<'_> {
    fn take_byte(&mut self) -> Option<u8> {
        let (&byte, rest) = self.rest.split_first()?;
        self.rest = rest;
        Some(byte)
    }
}

impl Iterator for Units<'_> {
    type Item = u16;

    fn next(&mut self) -> Option<u16> {
        let first = match self.odd.take() {
            Some(odd) => odd,
            None => self.take_byte()?,
        };
        if let Some(second) = self.take_byte() {
            Some(self.unit([first, second]))
        } else {
            self.odd = Some(first);
            None
        }
    }
}

impl Decoder {
    pub(super) fn new(
        encoding: Encoding,
        version: Version,
        max_document_bytes: Option<usize>,
    ) -> Self {
        Self {
            encoding,
            version,
            partial: [0; 4],
            partial_len: 0,
            odd: None,
            high: None,
            pending_cr: false,
            source_pos: 0,
            decoded_bytes: 0,
            max_document_bytes,
            scratch: String::new(),
        }
    }

    /// Decode `bytes`, normalise them and append to `text`, whose
    /// first byte is at document offset `base`.
    pub(super) fn push(
        &mut self,
        bytes: &[u8],
        text: &mut String,
        base: usize,
    ) -> Result<()> {
        match self.encoding {
            Encoding::Utf8 => self.push_utf8(bytes, text, base),
            Encoding::Utf16Be | Encoding::Utf16Le => {
                self.push_utf16(bytes, text, base)
            }
            Encoding::Latin1 => self.push_latin1(bytes, text, base),
        }
    }

    #[cfg(test)]
    pub(super) fn encoding(&self) -> Encoding {
        self.encoding
    }

    /// The source has ended: whatever was held back is judged.
    pub(super) fn finish(&mut self, text: &mut String) -> Result<()> {
        if self.odd.is_some() {
            return Err(Self::malformed(self.source_pos - 1));
        }
        if self.high.is_some() {
            return Err(Self::malformed(self.source_pos - 2));
        }
        if self.partial_len > 0 {
            return Err(Self::malformed(self.source_pos - self.partial_len));
        }
        // A held-back `\r` at the end of the document is a line ending
        // of its own.
        if self.pending_cr {
            self.pending_cr = false;
            text.push('\n');
        }
        Ok(())
    }

    /// Append text that is already decoded, normalising its line
    /// endings and checking its characters.
    pub(super) fn push_str(
        &mut self,
        s: &str,
        text: &mut String,
        base: usize,
    ) -> Result<()> {
        let total = self
            .decoded_bytes
            .checked_add(s.len())
            .ok_or_else(|| Error::new(ErrorKind::DocumentTooLarge, 0))?;
        crate::limits::check_bytes(
            total,
            self.max_document_bytes,
            ErrorKind::DocumentTooLarge,
            0,
        )?;
        self.decoded_bytes = total;
        let before = text.len();
        self.normalise_into(s, text);
        check_characters(&text[before..], self.version, base + before)
    }

    fn malformed(at: usize) -> Error {
        Error::new(ErrorKind::MalformedEncoding, at)
    }

    fn push_utf8(
        &mut self,
        bytes: &[u8],
        text: &mut String,
        base: usize,
    ) -> Result<()> {
        let Some(bytes) = self.complete_partial(bytes, text, base)? else {
            return Ok(());
        };
        let (whole, tail) = valid_prefix(bytes)
            .map_err(|at| Self::malformed(self.source_pos + at))?;
        self.source_pos += bytes.len();
        self.partial[..tail.len()].copy_from_slice(tail);
        self.partial_len = tail.len();
        self.push_str(whole, text, base)
    }

    /// Finish a character whose first bytes arrived last time, and
    /// return what is left of `bytes` after the bytes it took -- or
    /// `None` when the whole chunk went into it and it is still not
    /// complete.
    fn complete_partial<'b>(
        &mut self,
        bytes: &'b [u8],
        text: &mut String,
        base: usize,
    ) -> Result<Option<&'b [u8]>> {
        if self.partial_len == 0 {
            return Ok(Some(bytes));
        }
        let take = self.absorb(bytes);
        let Some(c) = self.held_char()? else {
            return Ok(None);
        };
        let mut buf = [0u8; 4];
        self.push_str(c.encode_utf8(&mut buf), text, base)?;
        Ok(Some(&bytes[take..]))
    }

    /// Take as many of `bytes` into the held sequence as it still
    /// needs, returning how many.
    fn absorb(&mut self, bytes: &[u8]) -> usize {
        let width = utf8_width(self.partial[0]);
        let take = (width - self.partial_len).min(bytes.len());
        self.partial[self.partial_len..][..take]
            .copy_from_slice(&bytes[..take]);
        self.partial_len += take;
        self.source_pos += take;
        take
    }

    /// The held sequence as a character once it is complete: `None`
    /// while it is not, and an error if it never can be.
    fn held_char(&mut self) -> Result<Option<char>> {
        let width = utf8_width(self.partial[0]);
        if self.partial_len < width {
            return Ok(None);
        }
        let c = core::str::from_utf8(&self.partial[..width])
            .ok()
            .and_then(|s| s.chars().next())
            .ok_or_else(|| Self::malformed(self.source_pos - width))?;
        self.partial_len = 0;
        Ok(Some(c))
    }

    fn push_utf16(
        &mut self,
        bytes: &[u8],
        text: &mut String,
        base: usize,
    ) -> Result<()> {
        // The offset of the first unit not yet turned into a character.
        let mut at = self.source_pos
            - usize::from(self.odd.is_some())
            - if self.high.is_some() { 2 } else { 0 };
        let mut units = Units {
            odd: self.odd.take(),
            rest: bytes,
            big_endian: self.encoding == Encoding::Utf16Be,
        };
        let mut scratch = core::mem::take(&mut self.scratch);
        scratch.clear();
        let mut outcome = Ok(());
        for unit in &mut units {
            if let Some(width) = self.unit(unit, &mut scratch) {
                at += width;
            } else {
                outcome = Err(Self::malformed(at));
                break;
            }
        }
        self.odd = units.odd;
        self.source_pos += bytes.len();
        let pushed = outcome.and_then(|()| self.push_str(&scratch, text, base));
        self.scratch = scratch;
        pushed
    }

    /// One UTF-16 code unit: the bytes it accounted for, or `None` for
    /// an unpaired surrogate, which is not a character in any encoding
    /// and is not silently replaced.
    fn unit(&mut self, unit: u16, out: &mut String) -> Option<usize> {
        match self.high.take() {
            Some(high) => surrogate_pair(high, unit, out),
            None if is_high_surrogate(unit) => {
                self.high = Some(unit);
                Some(0)
            }
            None => single_unit(unit, out),
        }
    }

    /// ISO-8859-1: every byte is the code point of the same value.
    fn push_latin1(
        &mut self,
        bytes: &[u8],
        text: &mut String,
        base: usize,
    ) -> Result<()> {
        let mut scratch = core::mem::take(&mut self.scratch);
        scratch.clear();
        scratch.extend(bytes.iter().map(|&b| char::from(b)));
        self.source_pos += bytes.len();
        let pushed = self.push_str(&scratch, text, base);
        self.scratch = scratch;
        pushed
    }

    /// XML 1.0 and 1.1 section 2.11, with the pair that may straddle
    /// a chunk held back. The same rewriting as
    /// `parser::normalize_line_endings`, without the intermediate
    /// copy.
    fn normalise_into(&mut self, s: &str, text: &mut String) {
        if s.is_empty() {
            return;
        }
        let mut rest = self.resume(s, text);
        while let Some(i) = rest.find(|c| self.is_terminator(c)) {
            text.push_str(&rest[..i]);
            match self.line_end(&rest[i..], text) {
                Some(after) => rest = after,
                None => return,
            }
        }
        text.push_str(rest);
    }

    /// Flush a `\r` held back from the last chunk, pairing it with
    /// whatever this one begins with.
    fn resume<'s>(&mut self, s: &'s str, text: &mut String) -> &'s str {
        if !self.pending_cr {
            return s;
        }
        self.pending_cr = false;
        text.push('\n');
        self.strip_pair_tail(s)
    }

    /// The terminator at the start of `rest`: what follows the line
    /// ending it makes, or `None` for a `\r` at the chunk's end, which
    /// is held back because its partner may be in the next one.
    fn line_end<'s>(
        &mut self,
        rest: &'s str,
        text: &mut String,
    ) -> Option<&'s str> {
        let c = rest.chars().next()?;
        let after = &rest[c.len_utf8()..];
        if c == '\r' && after.is_empty() {
            self.pending_cr = true;
            return None;
        }
        text.push('\n');
        Some(if c == '\r' {
            self.strip_pair_tail(after)
        } else {
            after
        })
    }

    fn is_terminator(&self, c: char) -> bool {
        c == '\r'
            || (self.version == Version::V11
                && (c == '\u{85}' || c == '\u{2028}'))
    }

    /// What may follow a `\r` as the second half of one line ending.
    fn strip_pair_tail<'s>(&self, s: &'s str) -> &'s str {
        if let Some(rest) = s.strip_prefix('\n') {
            return rest;
        }
        if self.version == Version::V11 {
            return s.strip_prefix('\u{85}').unwrap_or(s);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloc::vec::Vec;

    /// Decode `bytes` in chunks of `n`, as the reader would.
    fn decoded(
        bytes: &[u8],
        n: usize,
        encoding: Encoding,
        version: Version,
    ) -> Result<String> {
        let mut d = Decoder::new(encoding, version, None);
        let mut text = String::new();
        for chunk in bytes.chunks(n) {
            d.push(chunk, &mut text, 0)?;
        }
        d.finish(&mut text)?;
        Ok(text)
    }

    #[test]
    fn line_endings_normalise_the_same_at_every_chunk_size() {
        let doc = "a\r\nb\rc\nd\r\u{85}e\u{85}f\u{2028}g\r";
        for version in [Version::V10, Version::V11] {
            let expected = crate::parser::normalize_line_endings(doc, version)
                .into_owned();
            for n in 1..=doc.len() {
                let got = decoded(doc.as_bytes(), n, Encoding::Utf8, version)
                    .expect("valid");
                assert_eq!(got, expected, "{version:?} at chunk {n}");
            }
        }
    }

    #[test]
    fn utf16_decodes_the_same_at_every_chunk_size() {
        let doc = "<a>h\u{e9}llo \u{1F600}\r\n</a>";
        for big_endian in [true, false] {
            let bytes: Vec<u8> = doc
                .encode_utf16()
                .flat_map(|u| {
                    if big_endian {
                        u.to_be_bytes()
                    } else {
                        u.to_le_bytes()
                    }
                })
                .collect();
            let encoding = if big_endian {
                Encoding::Utf16Be
            } else {
                Encoding::Utf16Le
            };
            for n in 1..=bytes.len() {
                let got =
                    decoded(&bytes, n, encoding, Version::V10).expect("valid");
                assert_eq!(got, "<a>h\u{e9}llo \u{1F600}\n</a>", "chunk {n}");
            }
        }
    }

    #[test]
    fn decoding_errors_keep_the_offset_decode_reports() {
        let cases: [(&[u8], Encoding); 4] = [
            (b"<a>\xFF</a>", Encoding::Utf8),
            (b"<a>\xE2\x82</a>", Encoding::Utf8),
            (b"<\x00a\x00>\x00\x00\xDC<\x00", Encoding::Utf16Le),
            (b"<\x00a\x00>\x00\x3D\xD8", Encoding::Utf16Le),
        ];
        for (bytes, encoding) in cases {
            let expected = crate::encoding::decode(bytes)
                .expect_err("malformed")
                .offset;
            for n in 1..=bytes.len() {
                let got = decoded(bytes, n, encoding, Version::V10)
                    .expect_err("malformed");
                assert_eq!(got.kind, ErrorKind::MalformedEncoding);
                assert_eq!(got.offset, expected, "{bytes:?} at chunk {n}");
            }
        }
    }
}
