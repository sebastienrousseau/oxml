// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading a document from a byte source rather than a string.
//!
//! The property that matters is agreement: `Reader::from_reader` must
//! produce exactly what `Reader::new` produces on the same bytes, and
//! must do so **at every buffer size**. A bug here does not look like
//! a bug: it looks like a document that parses on one machine and not
//! another, or one that broke when a file grew. So the tests below
//! feed the same documents through a reader that returns one byte at
//! a time, which puts a boundary between every pair of characters.
//!
//! The contract has one qualification, stated in [`agree`]: an
//! illegal character or malformed byte sequence is found when its
//! chunk is decoded, so the streaming reader may have yielded events
//! before reporting it, and a malformed construct before it is
//! reported first.

use std::fmt::Write as _;
use std::io::{BufReader, Cursor, Read};

use oxml::stream::{Core, Event, Reader};
use oxml::{ErrorKind, Limits};

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

fn trickle(data: &[u8], n: usize) -> BufReader<Trickle> {
    BufReader::with_capacity(
        1,
        Trickle {
            data: data.to_vec(),
            at: 0,
            n,
        },
    )
}

/// What a read produced: every event as text, then the error that
/// ended it, if one did.
type Outcome = (Vec<String>, Option<(ErrorKind, usize)>);

fn describe(event: &Event) -> String {
    let qname = |prefix: &Option<String>, local: &str| match prefix {
        Some(p) => format!("{p}:{local}"),
        None => local.to_owned(),
    };
    match event {
        Event::StartElement {
            name,
            prefix,
            attributes,
            namespaces,
            ..
        } => {
            let mut s = format!("<{}", qname(prefix, &name.local));
            for (p, uri) in namespaces {
                let _ = write!(
                    s,
                    " xmlns{}={uri:?}",
                    p.as_ref().map_or(String::new(), |p| format!(":{p}"))
                );
            }
            for (n, v) in attributes {
                let _ = write!(s, " {{{:?}}}{}={v:?}", n.namespace, n.local);
            }
            s.push('>');
            s
        }
        Event::EndElement { name, prefix, .. } => {
            format!("</{}>", qname(prefix, &name.local))
        }
        Event::Text(t) => format!("text {t:?}"),
        Event::Comment(c) => format!("comment {c:?}"),
        Event::ProcessingInstruction { target, data } => {
            format!("pi {target} {data:?}")
        }
        _ => "?".to_owned(),
    }
}

/// Drain a reader to its outcome.
fn drain(mut next: impl FnMut() -> oxml::Result<Option<Event>>) -> Outcome {
    let mut out = Vec::new();
    loop {
        match next() {
            Ok(Some(event)) => out.push(describe(&event)),
            Ok(None) => return (out, None),
            Err(e) => return (out, Some((e.kind, e.offset))),
        }
    }
}

/// What `parse_bytes` would see: decode, then read in memory.
fn in_memory(bytes: &[u8]) -> Outcome {
    match oxml::encoding::decode(bytes) {
        Ok(text) => read(&text),
        Err(e) => (Vec::new(), Some((e.kind, e.offset))),
    }
}

/// `Reader::new` over text that is already decoded.
fn read(text: &str) -> Outcome {
    match Reader::new(text) {
        Ok(mut r) => drain(|| r.next_event()),
        Err(e) => (Vec::new(), Some((e.kind, e.offset))),
    }
}

fn streamed(bytes: &[u8], chunk: usize) -> Outcome {
    match Reader::from_reader(trickle(bytes, chunk)) {
        Ok(mut r) => drain(|| r.next_event()),
        Err(e) => (Vec::new(), Some((e.kind, e.offset))),
    }
}

/// An error the decoder finds rather than the scanner, which the
/// in-memory reader reports before yielding anything.
fn found_while_decoding(kind: &ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::IllegalCharacter(_) | ErrorKind::MalformedEncoding
    )
}

/// The length of a byte-order mark, which decoding offsets exclude.
fn bom_len(bytes: &[u8]) -> usize {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        3
    } else if bytes.starts_with(&[0xFE, 0xFF])
        || bytes.starts_with(&[0xFF, 0xFE])
    {
        2
    } else {
        0
    }
}

/// What `Reader::new` reports over the text that the bytes before
/// `refused` decode to, in that text's offsets, shrinking the prefix
/// past any earlier decoding error `decode` reported second.
fn before_refusal(
    bytes: &[u8],
    mut refused: usize,
) -> Option<(ErrorKind, usize)> {
    let bom = bom_len(bytes);
    loop {
        match oxml::encoding::decode(&bytes[..bom + refused]) {
            Ok(text) => return read(&text).1,
            Err(e) if e.offset < refused => refused = e.offset,
            Err(e) => return Some((e.kind, e.offset)),
        }
    }
}

/// Whether `got` is earlier than decoding's refusal at byte
/// `refused`. A decoding error is placed in input bytes, every other
/// error in decoded text, so each is compared in its own unit.
fn is_earlier(bytes: &[u8], refused: usize, got: &(ErrorKind, usize)) -> bool {
    match got {
        (ErrorKind::MalformedEncoding, at) => *at <= refused,
        (_, at) => {
            before_refusal(bytes, refused).is_some_and(|(kind, there)| {
                (&kind, there) == (&got.0, *at) || *at < there
            })
        }
    }
}

/// The contract between the two readers.
///
/// Exact agreement, except when the in-memory reader refused the
/// document for a character or byte it found while decoding: the
/// streaming reader decodes a chunk at a time, so it may have yielded
/// the events before that point, and a malformed construct before it
/// is reported first -- at an earlier offset, never a later one. A
/// decoding offset counts input bytes and any other offset counts
/// decoded text, which in UTF-16 differ for the same place.
fn agree(bytes: &[u8], chunk: usize, expected: &Outcome, got: &Outcome) {
    let label = || {
        format!(
            "{:?} at chunk size {chunk}",
            String::from_utf8_lossy(&bytes[..bytes.len().min(80)])
        )
    };
    match expected {
        (events, Some((kind, offset)))
            if events.is_empty() && found_while_decoding(kind) =>
        {
            let Some(reported) = &got.1 else {
                panic!(
                    "{}: streamed read accepted what decoding refused",
                    label()
                );
            };
            let legitimate = *reported == (kind.clone(), *offset)
                || match kind {
                    ErrorKind::MalformedEncoding => {
                        is_earlier(bytes, *offset, reported)
                    }
                    _ => {
                        reported.0 != ErrorKind::MalformedEncoding
                            && reported.1 < *offset
                    }
                };
            assert!(
                legitimate,
                "{}: expected {kind:?} at {offset}, got {reported:?}",
                label()
            );
        }
        _ => assert_eq!(got, expected, "{}", label()),
    }
}

/// The chunk sizes that matter: every small one, and a large one.
fn chunks() -> impl Iterator<Item = usize> {
    (1..=16).chain([8192])
}

fn assert_agreement_at_every_chunk(bytes: &[u8]) {
    let expected = in_memory(bytes);
    for chunk in chunks() {
        agree(bytes, chunk, &expected, &streamed(bytes, chunk));
    }
}

/// Well-formed documents, each a boundary the reader once got wrong.
fn documents() -> Vec<String> {
    let pad = "x".repeat(1100);
    let mut docs: Vec<String> = [
        "<a/>",
        // Whitespace before the root and no declaration: the reader
        // used to skip the whitespace, then measure the absent
        // declaration from offset 0 and slice behind its own cursor.
        " <a/>",
        "\n\t <a>x</a>",
        "<a><b>text</b></a>",
        "<a><b/><c/></a>",
        r#"<r xmlns:m="urn:u"><m:a k="1">x</m:a></r>"#,
        r#"<r xmlns="urn:d" xmlns:p="urn:p"><p:a p:x="1" y="2"/><b/></r>"#,
        "<a>one &amp; two</a>",
        "<a><![CDATA[<not markup>]]></a>",
        "<a>x &amp; <![CDATA[y]]> &#65; z<![CDATA[]]>w</a>",
        "<!-- before --><a/><!-- after -->",
        "<?xml version='1.0'?><a><b>1</b></a>",
        "<!DOCTYPE a [<!ENTITY e \"expanded\">]><a>&e;</a>",
        "<a>line one\nline two\r\nline three\rline four</a>",
        "<a x='  spaced  ' y=\"tab\there\"/>",
        "<a>\u{1F600} unicode \u{4e2d}\u{6587}</a>",
        // The prolog, a construct at a time.
        "<?xml version=\"1.0\"?>\n<!-- c -->\n<!DOCTYPE a [<!ENTITY e \"x\">]>\n<a>&e;</a>",
        "<?pi?> <!DOCTYPE a><a/>",
        "<?xml version='1.0'?> <!DOCTYPE a> <!DOCTYPE a><a/>",
        "<?xml version='1.0'?>\r\n<a/>\r\n",
        // A doctype whose delimiters lie inside comments, literals and
        // processing instructions.
        "<!DOCTYPE a [<!-- ] --> <!ENTITY e \"x>y\"> ]><a>&e;</a>",
        "<!DOCTYPE a SYSTEM \"x>y.dtd\"><a/>",
        "<!DOCTYPE a [<?pi ] > ?><!ENTITY e 'a]b'>]><a>&e;</a>",
        "<!DOCTYPE a PUBLIC \"-//p//\" \"s]>\" [<!ELEMENT a (#PCDATA)>]><a/>",
        // XML 1.1 line endings, paired and unpaired.
        "<?xml version=\"1.1\"?><a>x\r\u{85}y\u{2028}z\r\n</a>",
        "<?xml version=\"1.0\"?><a>x\r\u{85}y\u{2028}z</a>",
        "<?xml version=\"1.1\"?><a>\r</a>",
        // A tokenized attribute collapses as the tree parser collapses it.
        "<!DOCTYPE a [<!ATTLIST a x NMTOKEN #IMPLIED>]><a x=\"  p   q  \" y=\"  r \"/>",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    // Past the first kilobyte: a character-data run split by a
    // section, a long internal subset, whitespace before and after.
    docs.push(format!(
        "<r><p>{pad}</p><a>one <![CDATA[two]]> three</a></r>"
    ));
    docs.push(format!("<r><p>{pad}</p><a>&amp;<![CDATA[]]>&#x41;</a></r>"));
    docs.push(format!(
        "<!DOCTYPE a [<!-- {pad} --><!ENTITY e \"v\">]><a>&e;</a>"
    ));
    docs.push(format!(
        "<?xml version=\"1.0\"?>{}<!-- c --><a/>",
        " ".repeat(1100)
    ));
    docs.push(format!("<a/>{}<!-- after -->", " ".repeat(1100)));
    docs.push(format!("<a/>{}", "\n".repeat(1100)));
    docs
}

/// Documents both readers refuse, at the same place.
fn malformed() -> Vec<&'static str> {
    vec![
        "<a>",
        "<a></b>",
        "<a x='1' x='2'/>",
        "</a>",
        "<a/><b/>",
        "<a>&undefined;</a>",
        "<a x=1/>",
        "<a>\u{1}</a>",
        "<a>&x;\u{1}",
        " <?xml version='1.0'?><a/>",
        "<?xml version='1.0'?><?xml version='1.0'?><a/>",
        "<?xml?><a/>",
        " <?xml?><a/>",
        "<!DOCTYPE a [<!ENTITY e \"unterminated ]><a/>",
        "<a/><!DOCTYPE a>",
        "<!-- c --><!DOCTYPE a><!DOCTYPE a><a/>",
        "<a>]]></a>",
        "<a><![CDATA[unterminated</a>",
        "",
        "   ",
        "<",
        "<!-- no root -->",
        // Found by the stream_chunked fuzz target: it panicked.
        " </aa>  </a>",
        " </a>",
        // A declaration never closed by `?>` declares nothing, so its
        // `encoding` is not read: the streaming reader refused these
        // for the name while the in-memory one read them as UTF-8.
        "<?xml version='1.0' encoding='EBCDIC-CP-US'<a/>",
        "<?xml version='1.0' encoding='UTF~8'<a/>",
        // Found by the stream_chunked fuzz target: a reference with no
        // `;` before the next markup, or before the quote that closes
        // its value. The in-memory reader found one further on and
        // reported the span as an unknown entity's name; the
        // streaming reader sees only to the construct's end.
        "<g>am&></>>p;/></>>",
        "<g>6&<!ENT>b/+&amp;/><",
        "<g a=\"0&0\"[1]/><g>V<b/><b/ib4t&lt;",
    ]
}

/// Byte sequences in the encodings `parse_bytes` decodes.
fn encoded() -> Vec<Vec<u8>> {
    fn utf16(text: &str, big_endian: bool, bom: bool) -> Vec<u8> {
        let mut out = Vec::new();
        if bom {
            out.extend(if big_endian {
                [0xFE, 0xFF]
            } else {
                [0xFF, 0xFE]
            });
        }
        for unit in text.encode_utf16() {
            out.extend(if big_endian {
                unit.to_be_bytes()
            } else {
                unit.to_le_bytes()
            });
        }
        out
    }
    let doc = "<a x=\"\u{e9}\">h\u{e9}llo \u{1F600}\r\n</a>";
    vec![
        [b"\xEF\xBB\xBF".as_slice(), doc.as_bytes()].concat(),
        utf16(doc, true, true),
        utf16(doc, false, true),
        utf16(doc, true, false),
        utf16(doc, false, false),
        utf16(
            "<?xml version='1.0' encoding='UTF-16'?><a>x</a>",
            false,
            true,
        ),
        b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><a>caf\xE9</a>"
            .to_vec(),
        // Refused, by both: a mark and a declaration that disagree, an
        // encoding this crate cannot decode, bytes that are not UTF-8.
        b"\xEF\xBB\xBF<?xml version='1.0' encoding='iso-8859-1'?><a/>".to_vec(),
        b"<?xml version='1.0' encoding='EBCDIC-CP-US'?><a/>".to_vec(),
        b"<a>\xFF</a>".to_vec(),
        b"<a>\xE2\x82</a>".to_vec(),
        b"<a><b>fine</b>\xFF</a>".to_vec(),
        utf16("<a>\u{1F600}", false, true)[..9].to_vec(),
        // Found by the stream_chunked fuzz target: UTF-16BE by shape,
        // a NUL at input byte 12 (text offset 16) and an odd byte at
        // input byte 16. The harness compared the two 16s.
        b"\0<< `&a<\0\0m".to_vec(),
        // Found by the stream_chunked fuzz target: the encoding is
        // read from the declaration's bytes, as `decode` reads it. A
        // byte that is not UTF-8 before `encoding` moved the offset
        // the streaming reader reported; a name `decode` ignores
        // under a UTF-16 mark refused the document; and a UTF-8 mark
        // that contradicts the declaration was let through by
        // `decode` when a later byte was not UTF-8.
        b"<?xml \x8f encoding='UTF~8'?><a/>".to_vec(),
        utf16(
            "<?xml version='1.0' encoding='EBCDIC-CP-US'?><a>x</a>",
            false,
            true,
        ),
        b"\xEF\xBB\xBF<?xml version='1.0' encoding='iso-8859-1'?><a>\xE9</a>"
            .to_vec(),
    ]
}

/// The two entry points agree, at every buffer size that matters.
#[test]
fn a_streamed_read_matches_an_in_memory_one() {
    for doc in documents() {
        let expected = in_memory(doc.as_bytes());
        assert!(expected.1.is_none(), "{doc:?} is well-formed: {expected:?}");
        assert_agreement_at_every_chunk(doc.as_bytes());
    }
}

/// Malformed documents are refused identically, offsets included.
#[test]
fn a_streamed_read_fails_where_an_in_memory_one_does() {
    for doc in malformed() {
        let expected = in_memory(doc.as_bytes());
        assert!(expected.1.is_some(), "{doc:?} is malformed");
        assert_agreement_at_every_chunk(doc.as_bytes());
    }
}

/// Bytes are decoded as `parse_bytes` decodes them: by the mark, then
/// the declaration, then UTF-8 -- and chunked anywhere.
#[test]
fn a_streamed_read_decodes_what_parse_bytes_decodes() {
    for bytes in encoded() {
        assert_agreement_at_every_chunk(&bytes);
    }
    let bom = b"\xEF\xBB\xBF<a/>";
    assert_eq!(
        streamed(bom, 1).0,
        ["<a>", "</a>"],
        "the mark is not content"
    );
}

/// A random element of one to two kilobytes before the root moves
/// every later boundary, so a construct that was whole in the first
/// read is now split by it.
mod padded {
    use super::*;
    use proptest::prelude::*;

    /// Where the root element begins: after a declaration, if any.
    fn root_at(doc: &str) -> usize {
        if !doc.starts_with("<?xml ") && !doc.starts_with("<?xml\t") {
            return 0;
        }
        doc.find("?>").map_or(0, |i| i + 2)
    }

    /// Fixtures a padding element can be put in front of: those whose
    /// prolog is a declaration or nothing.
    fn paddable() -> Vec<String> {
        documents()
            .into_iter()
            .filter(|d| {
                let rest = &d[root_at(d)..];
                rest.trim_start().starts_with('<')
                    && !rest.trim_start().starts_with("<!")
                    && !rest.trim_start().starts_with("<?")
            })
            .collect()
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(48))]

        #[test]
        fn agreement_survives_a_random_prefix(
            prefix in "[a-zA-Z0-9 .,;]{1024,2048}",
            which in any::<prop::sample::Index>(),
            chunk in 1usize..=16,
        ) {
            let docs = paddable();
            let doc = &docs[which.index(docs.len())];
            let at = root_at(doc);
            let padded = format!(
                "{}<outer><pad>{prefix}</pad>{}</outer>",
                &doc[..at],
                &doc[at..]
            );
            let expected = in_memory(padded.as_bytes());
            prop_assert!(expected.1.is_none(), "{padded:?}: {expected:?}");
            agree(padded.as_bytes(), chunk, &expected, &streamed(padded.as_bytes(), chunk));
        }
    }
}

/// Every document in the W3C suite that carries a `DOCTYPE` reads the
/// same from bytes as from memory: the one corpus of doctypes large
/// enough to have every delimiter in every position.
///
/// Skipped when the suite is not checked out, unless
/// `OXML_REQUIRE_SUITE` is set, as the conformance crate does.
#[test]
fn every_doctype_in_the_conformance_suite_streams_as_it_parses() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("..")
        .join("conformance")
        .join("data")
        .join("xmlconf");
    if !dir.is_dir() {
        assert!(
            std::env::var_os("OXML_REQUIRE_SUITE").is_none(),
            "OXML_REQUIRE_SUITE is set and {} is missing",
            dir.display()
        );
        eprintln!("conformance suite not present; skipping");
        return;
    }
    let mut pending = vec![dir];
    let mut checked = 0usize;
    while let Some(dir) = pending.pop() {
        for entry in std::fs::read_dir(&dir).expect("readable") {
            let path = entry.expect("entry").path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "xml") {
                continue;
            }
            let bytes = std::fs::read(&path).expect("readable");
            if !bytes.windows(9).any(|w| w == b"<!DOCTYPE") {
                continue;
            }
            let expected = in_memory(&bytes);
            for chunk in [1, 7, 64] {
                agree(&bytes, chunk, &expected, &streamed(&bytes, chunk));
            }
            checked += 1;
        }
    }
    assert!(
        checked > 2_000,
        "only {checked} documents carried a DOCTYPE"
    );
}

/// A document far larger than any buffer, read through a small one.
#[test]
fn a_document_larger_than_the_buffer_is_read() {
    let mut doc = String::from("<catalogue>");
    for i in 0..20_000 {
        let _ = write!(doc, "<item id=\"i{i}\">value {i}</item>");
    }
    doc.push_str("</catalogue>");

    let mut reader =
        Reader::from_reader(trickle(doc.as_bytes(), 64)).expect("valid");
    let mut items = 0usize;
    while let Some(event) = reader.next_event().expect("valid") {
        if let Event::StartElement { name, .. } = event
            && name.local == "item"
        {
            items += 1;
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
#[test]
fn a_line_ending_split_across_a_read_is_one_ending() {
    let doc = "<a>one\r\ntwo\r\nthree</a>";
    let expected = in_memory(doc.as_bytes());
    for chunk in 1..12 {
        assert_eq!(
            streamed(doc.as_bytes(), chunk),
            expected,
            "a CRLF pair split at chunk size {chunk}"
        );
    }
}

/// A multi-byte character split across a read boundary survives.
#[test]
fn a_character_split_across_a_read_survives() {
    let doc = "<a>\u{1F600}</a>";
    let expected = in_memory(doc.as_bytes());
    for chunk in 1..8 {
        assert_eq!(
            streamed(doc.as_bytes(), chunk),
            expected,
            "an emoji split at chunk size {chunk}"
        );
    }
}

/// `standalone` is read from the text after line endings are
/// normalised, as the tree parser reads it.
#[test]
fn standalone_is_read_after_line_endings_are_normalised() {
    for separator in ['\u{85}', '\u{2028}', '\n'] {
        let doc = format!(
            "<?xml version=\"1.1\" standalone{separator}=\"yes\"?>\
             <!DOCTYPE a SYSTEM \"x.dtd\"><a>&e;</a>"
        );
        let expected = in_memory(doc.as_bytes());
        assert!(
            matches!(&expected.1, Some((ErrorKind::UnknownEntity(n), _)) if n == "e"),
            "separator {separator:?} gave {expected:?}"
        );
        let tree = oxml::parse(&doc).expect_err("undeclared entity");
        assert_eq!(Some((tree.kind, tree.offset)), expected.1, "{separator:?}");
        assert_eq!(streamed(doc.as_bytes(), 3), expected, "{separator:?}");
    }
}

/// A source that never ends: a prefix, then one byte forever.
struct Endless {
    prefix: Vec<u8>,
    at: usize,
    handed_out: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

impl Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        for slot in buf.iter_mut() {
            *slot = *self.prefix.get(self.at).unwrap_or(&b'x');
            self.at += 1;
        }
        let _ = self
            .handed_out
            .fetch_add(buf.len(), std::sync::atomic::Ordering::Relaxed);
        Ok(buf.len())
    }
}

/// A construct that never closes is refused once it passes
/// `max_construct_bytes`, instead of being buffered until memory runs
/// out.
#[test]
fn an_endless_construct_is_refused_within_the_construct_limit() {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    let limits = Limits::strict();
    let cap = limits.max_construct_bytes.expect("strict() bounds it");
    for prefix in [
        "<a>",
        "<!--",
        "<a><![CDATA[",
        "<?pi ",
        "<a b=\"",
        "<!DOCTYPE a [",
    ] {
        let handed_out = Arc::new(AtomicUsize::new(0));
        let source = BufReader::with_capacity(
            8192,
            Endless {
                prefix: prefix.as_bytes().to_vec(),
                at: 0,
                handed_out: Arc::clone(&handed_out),
            },
        );
        let mut reader =
            Reader::from_reader_with(source, limits).expect("prolog is fine");
        let error = loop {
            match reader.next_event() {
                Ok(Some(_)) => {}
                Ok(None) => panic!("{prefix:?} never ends"),
                Err(e) => break e,
            }
        };
        assert_eq!(error.kind, ErrorKind::ConstructTooLarge, "{prefix:?}");
        let swallowed = handed_out.load(Ordering::Relaxed);
        assert!(
            swallowed <= cap + 64 * 1024,
            "{prefix:?}: read {swallowed} bytes against a cap of {cap}"
        );
    }
}

/// The offset of a refused construct is where it began, in the
/// document rather than in the buffer.
#[test]
fn a_refused_construct_reports_where_it_began() {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;

    let mut limits = Limits::default();
    limits.max_construct_bytes = Some(16 * 1024);
    let prefix = "<a><b>fine</b><c>";
    let source = BufReader::new(Endless {
        prefix: prefix.as_bytes().to_vec(),
        at: 0,
        handed_out: Arc::new(AtomicUsize::new(0)),
    });
    let mut reader =
        Reader::from_reader_with(source, limits).expect("prolog is fine");
    let error = loop {
        match reader.next_event() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("never ends"),
            Err(e) => break e,
        }
    };
    assert_eq!(error.kind, ErrorKind::ConstructTooLarge);
    assert_eq!(error.offset, prefix.len(), "the text after <c>");
}

/// A single large construct is read in linear time.
#[test]
fn a_sixteen_megabyte_text_node_streams_through_a_small_buffer() {
    let body = "y".repeat(16 * 1024 * 1024);
    let doc = format!("<a>{body}</a>");
    let source = BufReader::with_capacity(8192, Cursor::new(doc.into_bytes()));
    let mut reader = Reader::from_reader(source).expect("prolog is fine");
    let mut text_len = 0usize;
    while let Some(event) = reader.next_event().expect("well-formed") {
        if let Event::Text(t) = event {
            text_len += t.len();
        }
    }
    assert_eq!(text_len, body.len());
}

/// An encoding error past the first buffer is reported at its offset
/// in the document, not in the window the reader happened to hold.
#[test]
fn an_encoding_error_past_the_first_buffer_keeps_its_document_offset() {
    let mut bytes =
        format!("<r><a>{}</a><b>{}", "y".repeat(20_000), "z".repeat(8_000))
            .into_bytes();
    let bad_at = bytes.len();
    bytes.push(0xFF);
    bytes.extend_from_slice(b"</b></r>");
    let mut reader = Reader::from_reader(BufReader::new(Cursor::new(bytes)))
        .expect("prolog is fine");
    let error = loop {
        match reader.next_event() {
            Ok(Some(_)) => {}
            Ok(None) => panic!("0xFF is not UTF-8"),
            Err(e) => break e,
        }
    };
    assert_eq!(error.kind, ErrorKind::MalformedEncoding);
    assert_eq!(error.offset, bad_at);
}

/// A source that fails part-way leaves the reader failed, as a parse
/// error does, rather than resuming from wherever it stopped.
#[test]
fn a_failing_source_fails_the_reader_for_good() {
    struct Flaky {
        data: &'static [u8],
        at: usize,
        interrupted: bool,
    }
    impl Read for Flaky {
        fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
            if !self.interrupted {
                self.interrupted = true;
                return Err(std::io::Error::from(
                    std::io::ErrorKind::Interrupted,
                ));
            }
            if self.at >= self.data.len() {
                return Err(std::io::Error::other("disk on fire"));
            }
            buf[0] = self.data[self.at];
            self.at += 1;
            Ok(1)
        }
    }
    let source = BufReader::with_capacity(
        1,
        Flaky {
            data: b"<a><b/>",
            at: 0,
            interrupted: false,
        },
    );
    let mut reader =
        Reader::from_reader(source).expect("an interruption is retried");
    assert!(matches!(
        reader.next_event(),
        Ok(Some(Event::StartElement { .. }))
    ));
    assert!(matches!(
        reader.next_event(),
        Ok(Some(Event::StartElement { .. }))
    ));
    assert!(matches!(
        reader.next_event(),
        Ok(Some(Event::EndElement { .. }))
    ));
    let error = reader.next_event().expect_err("the source failed");
    assert!(
        matches!(error.kind, ErrorKind::Io(ref m) if m.contains("disk on fire"))
    );
    assert_eq!(error.offset, 7, "where the next byte would have landed");
    assert!(
        matches!(reader.next_event(), Ok(None)),
        "and it stays failed"
    );
}

/// A reader is `Send` whenever its source is, so a document can be
/// read on another thread; the sans-I/O core always is.
#[test]
fn readers_are_send_when_their_source_is() {
    fn assert_send<T: Send>() {}
    assert_send::<Reader>();
    assert_send::<Reader<'static, Cursor<Vec<u8>>>>();
    assert_send::<Reader<'static, BufReader<std::fs::File>>>();
    assert_send::<Core<'static>>();

    let doc = b"<a><b>moved</b></a>".to_vec();
    let reader = Reader::from_reader(Cursor::new(doc)).expect("valid");
    let texts = std::thread::spawn(move || {
        reader
            .filter_map(|e| match e {
                Ok(Event::Text(t)) => Some(t),
                _ => None,
            })
            .collect::<Vec<_>>()
    })
    .join()
    .expect("thread");
    assert_eq!(texts, ["moved"]);
}

/// The reader is an iterator that ends after the document, or after
/// the first error -- and never resumes.
#[test]
fn the_reader_iterates_until_the_document_or_an_error_ends_it() {
    let good: Vec<Event> =
        Reader::from_reader(Cursor::new(b"<a>x</a>".to_vec()))
            .expect("valid")
            .collect::<oxml::Result<_>>()
            .expect("well-formed");
    assert_eq!(good.len(), 3);

    let mut items =
        Reader::from_reader(Cursor::new(b"<a></b>".to_vec())).expect("prolog");
    assert!(items.next().is_some_and(|e| e.is_ok()));
    assert!(items.next().is_some_and(|e| e.is_err()));
    assert!(items.next().is_none(), "nothing after the error");
}
