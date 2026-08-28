// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document from a byte source rather than a string.
//!
//! The property that matters is agreement: `Reader::from_reader` must
//! produce exactly what `Reader::new` produces on the same bytes, and
//! must do so **at every buffer size**. A bug here does not look like
//! a bug — it looks like a document that parses on one machine and not
//! another, or one that broke when a file grew. So the tests below
//! feed the same documents through a reader that returns one byte at a
//! time, which puts a boundary between every pair of characters.

use std::fmt::Write as _;
use std::io::{BufReader, Read};

use oxml::stream::{Event, Reader};

/// A reader that returns at most `n` bytes per call.
///
/// `BufReader` would hide the boundaries by refilling underneath;
/// this puts one exactly where it is least convenient.
struct Trickle {
    data: Vec<u8>,
    at: usize,
    n: usize,
}

impl Read for Trickle {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let take = self.n.min(buf.len()).min(self.data.len() - self.at);
        buf[..take].copy_from_slice(&self.data[self.at..self.at + take]);
        self.at += take;
        Ok(take)
    }
}

/// `from_reader` takes `R: BufRead + 'static`, so the reader owns its
/// bytes. A caller with a `String` wraps it in `std::io::Cursor`; a
/// caller with a file has one already.
fn trickle(data: &str, n: usize) -> BufReader<Trickle> {
    BufReader::with_capacity(
        1,
        Trickle {
            data: data.as_bytes().to_vec(),
            at: 0,
            n,
        },
    )
}

/// Every event, as text, so two readers can be compared.
fn described(
    mut next: impl FnMut() -> Result<Option<Event>, oxml::Error>,
) -> Result<Vec<String>, oxml::Error> {
    let mut out = Vec::new();
    while let Some(event) = next()? {
        out.push(match event {
            Event::StartElement { name, attributes } => {
                let mut s = format!("<{}", name.local);
                for (n, v) in &attributes {
                    let _ = write!(s, " {}={v:?}", n.local);
                }
                s.push('>');
                s
            }
            Event::EndElement { name } => format!("</{}>", name.local),
            Event::Text(t) => format!("text {t:?}"),
            Event::Comment(c) => format!("comment {c:?}"),
            Event::ProcessingInstruction { target, data } => {
                format!("pi {target} {data:?}")
            }
            _ => "?".to_owned(),
        });
    }
    Ok(out)
}

fn in_memory(doc: &str) -> Result<Vec<String>, oxml::Error> {
    let mut r = Reader::new(doc)?;
    described(move || r.next_event())
}

fn from_bytes(doc: &str, chunk: usize) -> Result<Vec<String>, oxml::Error> {
    let mut r = Reader::from_reader(trickle(doc, chunk))?;
    described(move || r.next_event())
}

const DOCS: &[&str] = &[
    "<a/>",
    "<a><b>text</b></a>",
    "<a><b/><c/></a>",
    r#"<r xmlns:m="urn:u"><m:a k="1">x</m:a></r>"#,
    "<a>one &amp; two</a>",
    "<a><![CDATA[<not markup>]]></a>",
    "<!-- before --><a/><!-- after -->",
    "<?xml version='1.0'?><a><b>1</b></a>",
    "<!DOCTYPE a [<!ENTITY e \"expanded\">]><a>&e;</a>",
    "<a>line one\nline two\r\nline three\rline four</a>",
    "<a x='  spaced  ' y=\"tab\there\"/>",
    "<a>\u{1F600} unicode \u{4e2d}\u{6587}</a>",
];

/// The two entry points agree, at every buffer size that matters.
#[test]
fn a_streamed_read_matches_an_in_memory_one() {
    for doc in DOCS {
        let expected = in_memory(doc).unwrap_or_else(|e| {
            panic!("in-memory reader refused {doc:?}: {e}")
        });
        // One byte at a time puts a boundary between every pair of
        // characters, including inside a multi-byte one.
        for chunk in [1, 2, 3, 7, 64, 8192] {
            let got = from_bytes(doc, chunk).unwrap_or_else(|e| {
                panic!("streamed reader refused {doc:?} at chunk {chunk}: {e}")
            });
            assert_eq!(got, expected, "{doc:?} at chunk size {chunk}");
        }
    }
}

/// Malformed documents are refused identically, offsets included.
#[test]
fn a_streamed_read_fails_where_an_in_memory_one_does() {
    for doc in [
        "<a>",
        "<a></b>",
        "<a x='1' x='2'/>",
        "</a>",
        "<a/><b/>",
        "<a>&undefined;</a>",
        "<a x=1/>",
    ] {
        let memory = in_memory(doc).expect_err("invalid");
        for chunk in [1, 3, 8192] {
            let streamed = from_bytes(doc, chunk).expect_err("invalid");
            assert_eq!(
                (&streamed.kind, streamed.offset),
                (&memory.kind, memory.offset),
                "{doc:?} at chunk {chunk}: offsets are the document's, \
                 not the buffer's"
            );
        }
    }
}

/// A document far larger than any buffer, read through a small one.
#[test]
fn a_document_larger_than_the_buffer_is_read() {
    use std::fmt::Write as _;

    let mut doc = String::from("<catalogue>");
    for i in 0..20_000 {
        let _ = write!(doc, "<item id=\"i{i}\">value {i}</item>");
    }
    doc.push_str("</catalogue>");

    let mut reader = Reader::from_reader(trickle(&doc, 64)).expect("valid");
    let mut items = 0usize;
    while let Some(event) = reader.next_event().expect("valid") {
        if let Event::StartElement { name, .. } = event {
            if name.local == "item" {
                items += 1;
            }
        }
    }
    assert_eq!(
        items,
        20_000,
        "{} KB read 64 bytes at a time",
        doc.len() / 1024
    );
}

/// A `\r\n` split across a read boundary is one line ending, not two.
///
/// This is the failure that only appears at one buffer size: the pair
/// normalises to a single `\n`, so emitting the `\r` as soon as it
/// arrives produces text that differs from the in-memory reader's by
/// one character, and only when the boundary lands between them.
#[test]
fn a_line_ending_split_across_a_read_is_one_ending() {
    let doc = "<a>one\r\ntwo\r\nthree</a>";
    let expected = in_memory(doc).expect("valid");
    for chunk in 1..12 {
        assert_eq!(
            from_bytes(doc, chunk).expect("valid"),
            expected,
            "a CRLF pair split at chunk size {chunk}"
        );
    }
}

/// A multi-byte character split across a read boundary survives.
#[test]
fn a_character_split_across_a_read_survives() {
    // Four bytes, so a boundary can fall in three places inside it.
    let doc = "<a>\u{1F600}</a>";
    let expected = in_memory(doc).expect("valid");
    for chunk in 1..8 {
        assert_eq!(
            from_bytes(doc, chunk).expect("valid"),
            expected,
            "an emoji split at chunk size {chunk}"
        );
    }
}

/// `standalone` is read from the text after line endings are
/// normalised, as the tree parser reads it.
///
/// XML 1.1 turns NEL and U+2028 into line feeds, so they are
/// whitespace only after normalisation. Reading the declaration from
/// the raw input instead made `standalone="yes"` invisible whenever
/// one of them separated `standalone` from its `=`, and the flag is
/// what withdraws the excuse for an entity the external subset might
/// have declared. The document below therefore parsed *successfully*,
/// silently dropping content it had asked for, on one entry point and
/// failed on the other two.
#[test]
fn standalone_is_read_after_line_endings_are_normalised() {
    for separator in ['\u{85}', '\u{2028}', '\n'] {
        let doc = format!(
            "<?xml version=\"1.1\" standalone{separator}=\"yes\"?>\
             <!DOCTYPE a SYSTEM \"x.dtd\"><a>&e;</a>"
        );
        let streamed = in_memory(&doc).expect_err("undeclared entity");
        assert!(
            matches!(streamed.kind, oxml::ErrorKind::UnknownEntity(ref n) if n == "e"),
            "separator {separator:?} gave {streamed:?}"
        );
        // The reader and the tree parser must agree about it.
        let tree = oxml::parse(&doc).expect_err("undeclared entity");
        assert_eq!(tree.kind, streamed.kind, "separator {separator:?}");
        assert_eq!(
            from_bytes(&doc, 3).expect_err("undeclared entity").kind,
            streamed.kind,
            "separator {separator:?}"
        );
    }
}
