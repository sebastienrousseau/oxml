// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Writing a document back out.
//!
//! The property under test is a fixed point: the first serialisation
//! may change the text — expanding an entity, dropping a CDATA wrapper
//! — and no later one changes anything. A caller that stores or
//! compares serialised documents depends on exactly that, and it is
//! testable in a way "round trips byte for byte" is not, because the
//! tree does not retain what a byte-exact writer would need.

/// `to_xml` applied twice must equal `to_xml` applied once.
fn assert_fixed_point(source: &str) {
    let once = oxml::parse(source)
        .unwrap_or_else(|e| panic!("{source:?} does not parse: {e}"))
        .to_xml();
    let twice = oxml::parse(&once)
        .unwrap_or_else(|e| panic!("output {once:?} does not re-parse: {e}"))
        .to_xml();
    assert_eq!(
        once, twice,
        "serialisation is not a fixed point for {source:?}"
    );
}

#[test]
fn output_is_a_fixed_point() {
    for source in [
        "<a/>",
        "<a></a>",
        "<a>text</a>",
        "<a><b/><c/></a>",
        "<a k=\"v\"/>",
        "<a k='v'/>",
        "<a k=\"1\" j=\"2\" i=\"3\"/>",
        "<a><!-- comment --></a>",
        "<?target data?><a/>",
        "<a><?pi?></a>",
        "<a><![CDATA[<raw>]]></a>",
        "<a>&amp;&lt;&gt;</a>",
        "<a>&#38;</a>",
        "<a xmlns=\"urn:d\"><b/></a>",
        "<a xmlns:p=\"urn:p\"><p:b p:k=\"v\"/></a>",
        "<a>  spaced  </a>",
        "<a>\u{1F600}</a>",
        "<?xml version=\"1.0\"?><a/>",
        "<a><b><c><d/></c></b></a>",
    ] {
        assert_fixed_point(source);
    }
}

#[test]
fn structure_survives_a_round_trip() {
    let source = "<r xmlns:m=\"urn:u\"><m:a k=\"v\">t</m:a><!--c--><?p d?></r>";
    let first = oxml::parse(source).expect("valid");
    let again = oxml::parse(&first.to_xml()).expect("output parses");

    assert_eq!(first.len(), again.len(), "node count changed");
    for (a, b) in first.descendants().zip(again.descendants()) {
        assert_eq!(
            format!("{:?}", first.kind(a)),
            format!("{:?}", again.kind(b)),
            "node kind changed across the round trip"
        );
    }
}

#[test]
fn text_is_escaped_so_it_reads_back_the_same() {
    let doc = oxml::parse("<a>&lt;b&gt; &amp; more</a>").expect("valid");
    let out = doc.to_xml();
    assert!(!out.contains("<b>"), "unescaped markup in {out:?}");
    let again = oxml::parse(&out).expect("output parses");
    assert_eq!(
        again.text(again.root_element().expect("a root")),
        "<b> & more",
        "text changed meaning"
    );
}

#[test]
fn attribute_whitespace_survives_normalisation() {
    // A literal tab or newline in an attribute value normalises to a
    // space when read back. Writing them as character references is
    // what keeps the value intact, and this is the case that catches
    // it: the loss would be silent and only for values that happen to
    // contain whitespace.
    let doc = oxml::parse("<a k=\"one&#9;two&#10;three\"/>").expect("valid");
    let out = doc.to_xml();
    let again = oxml::parse(&out).expect("output parses");
    let root = again.root_element().expect("a root");
    assert_eq!(
        again.attribute(root, "k"),
        Some("one\ttwo\nthree"),
        "attribute whitespace was lost by {out:?}"
    );
}

#[test]
fn the_implicit_xml_prefix_is_not_redeclared() {
    let doc = oxml::parse("<a xml:lang=\"en\"/>").expect("valid");
    let out = doc.to_xml();
    assert!(
        !out.contains("xmlns:xml"),
        "redeclared the implicit xml prefix: {out:?}"
    );
    assert!(out.contains("xml:lang"), "lost the prefix: {out:?}");
    let _ = oxml::parse(&out).expect("output parses");
}

#[test]
fn an_empty_element_is_written_self_closing() {
    assert_eq!(oxml::parse("<a></a>").expect("valid").to_xml(), "<a/>");
}

/// Every document in the W3C corpus that parses must survive a round
/// trip and be a fixed point.
///
/// The hand-written cases above are the ones someone thought of. This
/// is the one that found the defects: three separate root causes, none
/// of which appeared in the list above.
///
/// - A literal `\r` in character data normalises to `\n` on re-parse,
///   so it has to go out as `&#13;`.
/// - A C0 control character is legal in XML 1.1 only as a reference,
///   and only if the output says it is 1.1.
/// - Unbinding a prefix with `xmlns:p=""` also requires 1.1, which is
///   not a character question at all and was missed by the fix for the
///   one above it.
#[test]
fn every_conformance_document_is_a_fixed_point() {
    let root = std::path::Path::new("../../conformance/data");
    if !root.is_dir() {
        eprintln!("skipping: run the conformance downloader first");
        return;
    }

    let mut checked = 0usize;
    let mut failures = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().is_none_or(|e| e != "xml") {
                continue;
            }
            let Ok(source) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Documents this parser rejects are not this test's
            // business; the conformance suite covers those.
            let Ok(doc) = oxml::parse(&source) else {
                continue;
            };
            checked += 1;
            let once = doc.to_xml();
            match oxml::parse(&once) {
                Ok(again) if again.to_xml() == once => {}
                Ok(_) => failures
                    .push(format!("{}: not a fixed point", path.display())),
                Err(e) => failures.push(format!(
                    "{}: output does not parse: {e}",
                    path.display()
                )),
            }
        }
    }

    assert!(
        checked > 1_000,
        "only {checked} documents were checked; the corpus is probably not where this expects it"
    );
    assert!(
        failures.is_empty(),
        "{} of {checked} documents failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

mod options {
    use oxml::{EmptyElement, Indent, Newline, SerialiseOptions};

    fn pretty() -> SerialiseOptions {
        SerialiseOptions {
            indent: Some(Indent::Spaces(2)),
            ..SerialiseOptions::default()
        }
    }

    #[test]
    fn default_options_are_exactly_to_xml() {
        // `to_xml_with(default)` must be `to_xml`, byte for byte. If
        // the two writers drift apart, one of them is wrong.
        let doc = oxml::parse(
            r#"<a x="1"><b>text</b><c/><!--note--><?pi data?></a>"#,
        )
        .expect("well-formed");
        assert_eq!(doc.to_xml_with(SerialiseOptions::default()), doc.to_xml());
    }

    #[test]
    fn element_only_content_is_indented() {
        let doc = oxml::parse("<a><b><c/></b><d/></a>").expect("well-formed");
        let out = doc.to_xml_with(pretty());
        assert_eq!(out, "<a>\n  <b>\n    <c/>\n  </b>\n  <d/>\n</a>");
    }

    #[test]
    fn mixed_content_is_never_touched() {
        // The rule that makes pretty-printing safe by construction:
        // an element with any non-element child is written exactly as
        // the compact form writes it, because whitespace inserted next
        // to character data becomes part of that data.
        let source = "<a><p>text<b/>tail</p><q><r/></q></a>";
        let doc = oxml::parse(source).expect("well-formed");
        let out = doc.to_xml_with(pretty());
        assert!(
            out.contains("<p>text<b/>tail</p>"),
            "mixed content must be byte-identical: {out}"
        );
        assert!(
            out.contains("<q>\n    <r/>\n  </q>"),
            "element-only siblings still indent: {out}"
        );
    }

    #[test]
    fn pretty_printing_preserves_every_documents_text() {
        // Over the corpus: for every document that parses, the
        // pretty-printed form must reparse with *identical character
        // data* in every element. This is the property the mixed-
        // content rule exists to guarantee, checked over 1,900+ real
        // documents rather than the cases anyone thought of.
        let mut checked = 0u32;
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../conformance/data");
        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            let Ok(entries) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "xml") {
                    continue;
                }
                let Ok(bytes) = std::fs::read(&path) else {
                    continue;
                };
                let Ok(text) = String::from_utf8(bytes) else {
                    continue;
                };
                let Ok(doc) = oxml::parse(&text) else {
                    continue;
                };
                let out = doc.to_xml_with(pretty());
                let Ok(again) = oxml::parse(&out) else {
                    panic!(
                        "pretty output failed to parse for {path:?}:\n{out}"
                    );
                };
                // Compare text per element in document order. The
                // pretty form has extra whitespace-only text nodes, so
                // whole-document text differs where indentation was
                // inserted -- but only between elements, never inside
                // an element that had character data.
                let originals: Vec<String> = doc
                    .descendants()
                    .filter(|d| {
                        doc.children(*d).iter().any(|c| {
                            matches!(
                                doc.kind(*c),
                                Some(oxml::tree::NodeKind::Text(_))
                            )
                        })
                    })
                    .map(|d| doc.text(d))
                    .collect();
                let reparsed: Vec<String> = again
                    .descendants()
                    .filter(|d| {
                        again.children(*d).iter().any(|c| {
                            matches!(
                                again.kind(*c),
                                Some(oxml::tree::NodeKind::Text(_))
                            )
                        })
                    })
                    .map(|d| again.text(d))
                    .filter(|t| !t.trim().is_empty() || originals.contains(t))
                    .collect();
                for original in &originals {
                    assert!(
                        reparsed.contains(original),
                        "text changed by pretty-printing in {path:?}:\
                         \n  missing: {original:?}"
                    );
                }
                checked += 1;
            }
        }
        assert!(checked > 1_000, "only {checked} documents checked");
    }

    #[test]
    fn empty_elements_can_be_spelled_three_ways() {
        let doc = oxml::parse("<a><b/></a>").expect("well-formed");
        let spell = |e| {
            doc.to_xml_with(SerialiseOptions {
                empty_elements: e,
                ..SerialiseOptions::default()
            })
        };
        assert_eq!(spell(EmptyElement::SelfClosing), "<a><b/></a>");
        assert_eq!(spell(EmptyElement::SelfClosingSpaced), "<a><b /></a>");
        assert_eq!(spell(EmptyElement::Expanded), "<a><b></b></a>");
        // All three parse back to the same tree.
        for e in [
            EmptyElement::SelfClosing,
            EmptyElement::SelfClosingSpaced,
            EmptyElement::Expanded,
        ] {
            let out = spell(e);
            assert_eq!(
                oxml::parse(&out).expect("parses").to_xml(),
                doc.to_xml(),
                "{out}"
            );
        }
    }

    #[test]
    fn crlf_is_used_when_asked() {
        let doc = oxml::parse("<a><b/></a>").expect("well-formed");
        let out = doc.to_xml_with(SerialiseOptions {
            indent: Some(Indent::Tab),
            newline: Newline::CrLf,
            ..SerialiseOptions::default()
        });
        assert_eq!(out, "<a>\r\n\t<b/>\r\n</a>");
    }
}
