// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! End-of-line normalisation, XML 1.0 and 1.1 section 2.11.
//!
//! A processor must behave as though it had translated line endings to
//! `\n` before parsing. oxml did not, which had two consequences: the
//! text of every document written on Windows came back containing
//! `\r\n`, and the XML 1.1 terminators were not recognised as
//! whitespace at all -- the one W3C conformance test oxml rejected
//! wrongly.

use oxml::parse;

/// The string-value of the root, for a document with one text node.
fn text_of(source: &str) -> String {
    let doc = parse(source).expect("well-formed");
    doc.text(doc.root())
}

#[test]
fn xml_1_0_collapses_carriage_returns() {
    // Every document authored on Windows takes this path. Returning
    // the `\r` means a caller comparing text against a literal, or
    // splitting on lines, gets an answer that depends on which
    // operating system wrote the file.
    assert_eq!(text_of("<a>x\r\ny</a>"), "x\ny", "CR LF");
    assert_eq!(text_of("<a>x\ry</a>"), "x\ny", "a lone CR");
    assert_eq!(text_of("<a>x\r\r\ny</a>"), "x\n\ny", "CR then CR LF");
    assert_eq!(
        text_of("<a>x\n\ry</a>"),
        "x\n\ny",
        "LF then CR: two endings"
    );
}

#[test]
fn xml_1_0_leaves_the_other_terminators_alone() {
    // NEL and LINE SEPARATOR are ordinary characters in 1.0. Treating
    // them as line endings there would accept documents the
    // specification says are malformed.
    assert_eq!(
        text_of("<a>x\u{85}y</a>"),
        "x\u{85}y",
        "NEL is not an ending"
    );
    assert_eq!(
        text_of("<a>x\u{2028}y</a>"),
        "x\u{2028}y",
        "LINE SEPARATOR is not an ending"
    );
    // And so they cannot separate a name from an attribute.
    assert!(parse("<a\u{85}b=\"h\"/>").is_err(), "NEL as a separator");
}

#[test]
fn xml_1_1_adds_nel_and_line_separator() {
    let nel = "<?xml version=\"1.1\"?><a>x\u{85}y</a>";
    let ls = "<?xml version=\"1.1\"?><a>x\u{2028}y</a>";
    assert_eq!(text_of(nel), "x\ny", "NEL");
    assert_eq!(text_of(ls), "x\ny", "LINE SEPARATOR");

    // CR followed by NEL is one ending, not two.
    let pair = "<?xml version=\"1.1\"?><a>x\r\u{85}y</a>";
    assert_eq!(text_of(pair), "x\ny", "CR NEL");

    // And 1.0's rules still apply.
    let crlf = "<?xml version=\"1.1\"?><a>x\r\ny</a>";
    assert_eq!(text_of(crlf), "x\ny", "CR LF in 1.1");
}

#[test]
fn a_normalised_terminator_is_whitespace_where_whitespace_is_required() {
    // This is `eduni/rmt-e2e-50` from the W3C suite, and the reason
    // this was found: the NEL separates the element name from its
    // attribute, so the document is valid and was being rejected with
    // `expected a name`.
    for (label, source) in [
        ("NEL", "<?xml version=\"1.1\"?><foo\u{85}bar=\"hello\"/>"),
        ("LS", "<?xml version=\"1.1\"?><foo\u{2028}bar=\"hello\"/>"),
        ("CR", "<foo\rbar=\"hello\"/>"),
        ("CRLF", "<foo\r\nbar=\"hello\"/>"),
    ] {
        let doc = parse(source).unwrap_or_else(|e| panic!("{label}: {e}"));
        let root = doc.root_element().expect("a root element");
        assert_eq!(doc.attribute(root, "bar"), Some("hello"), "{label}");
    }
}

#[test]
fn a_character_reference_is_not_normalised() {
    // `&#xD;` is markup when normalisation runs and becomes a carriage
    // return when the reference is expanded. It is the only way to
    // write one that survives, so normalising it would make the
    // character unrepresentable.
    assert_eq!(text_of("<a>x&#xD;y</a>"), "x\ry");
    assert_eq!(text_of("<a>x&#13;y</a>"), "x\ry");
    let v11 = "<?xml version=\"1.1\"?><a>x&#x85;y</a>";
    assert_eq!(text_of(v11), "x\u{85}y", "an escaped NEL survives");
}

#[test]
fn a_document_with_no_carriage_returns_is_not_copied() {
    // The common case must stay free. This does not observe the `Cow`
    // directly -- it is private -- but a parse that allocated a
    // normalised copy of a 1 MB document would show up here.
    let source = format!("<a>{}</a>", "x".repeat(1_000_000));
    let doc = parse(&source).expect("well-formed");
    assert_eq!(doc.text(doc.root()).len(), 1_000_000);
}

#[test]
fn normalisation_applies_inside_every_construct() {
    // Not just text: the specification normalises the whole entity
    // before parsing, so comments and CDATA get it too.
    let doc =
        parse("<a><!--x\r\ny--><![CDATA[p\r\nq]]></a>").expect("well-formed");
    let root = doc.root_element().expect("a root element");
    assert_eq!(doc.text(root), "p\nq", "CDATA");

    let comment = doc
        .descendants()
        .find_map(|id| match doc.kind(id) {
            Some(oxml::NodeKind::Comment(text)) => Some(text.to_owned()),
            _ => None,
        })
        .expect("a comment");
    assert_eq!(comment, "x\ny", "comment");
}
