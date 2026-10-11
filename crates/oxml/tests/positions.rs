// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `Document::range`: where each parsed node came from.
//!
//! An editor or a linter needs to point at the source of a node; the
//! tree used to keep text ranges internally and expose none of them.

use oxml::{Document, NodeKind, parse};

const SRC: &str =
    "<?pi data?><!-- c --><r a=\"1\"><x>t&amp;u</x><y/><!--in--><?p q?></r>";

fn find(doc: &Document, kind: &str) -> Vec<oxml::NodeId> {
    doc.nodes()
        .filter(|&n| match doc.kind(n) {
            Some(NodeKind::Element { .. }) => kind == "element",
            Some(NodeKind::Text(_)) => kind == "text",
            Some(NodeKind::Comment(_)) => kind == "comment",
            Some(NodeKind::ProcessingInstruction { .. }) => kind == "pi",
            _ => false,
        })
        .collect()
}

#[test]
fn an_element_spans_its_start_tag_to_its_end_tag() {
    let doc = parse(SRC).expect("well-formed");
    let root = doc.root_element().expect("root element");
    assert_eq!(
        &SRC[doc.range(root).expect("range")],
        "<r a=\"1\"><x>t&amp;u</x><y/><!--in--><?p q?></r>"
    );
    let [x, y]: [oxml::NodeId; 2] = doc.children(root)[..2]
        .try_into()
        .expect("two elements first");
    assert_eq!(&SRC[doc.range(x).expect("range")], "<x>t&amp;u</x>");
    assert_eq!(&SRC[doc.range(y).expect("range")], "<y/>");
}

#[test]
fn text_comments_and_instructions_span_the_construct_as_written() {
    let doc = parse(SRC).expect("well-formed");
    let text = find(&doc, "text");
    assert_eq!(&SRC[doc.range(text[0]).expect("range")], "t&amp;u");
    let comments = find(&doc, "comment");
    assert_eq!(&SRC[doc.range(comments[0]).expect("range")], "<!-- c -->");
    assert_eq!(&SRC[doc.range(comments[1]).expect("range")], "<!--in-->");
    let pis = find(&doc, "pi");
    assert_eq!(&SRC[doc.range(pis[0]).expect("range")], "<?pi data?>");
    assert_eq!(&SRC[doc.range(pis[1]).expect("range")], "<?p q?>");
}

#[test]
fn the_root_spans_the_whole_input() {
    let doc = parse(SRC).expect("well-formed");
    assert_eq!(doc.range(doc.root()), Some(0..SRC.len()));
}

#[test]
fn offsets_match_the_error_offsets_after_line_ending_normalisation() {
    // `\r\n` becomes `\n`, so the second element starts one byte
    // earlier than in the caller's string; the range indexes the
    // normalised text, the same text error offsets index.
    let src = "<r>\r\n<a/>\r\n</r>";
    let doc = parse(src).expect("well-formed");
    let a = doc.children(doc.root_element().expect("r"))[1];
    assert_eq!(doc.range(a), Some(4..8));
    let normalised = src.replace("\r\n", "\n");
    assert_eq!(&normalised[doc.range(a).expect("range")], "<a/>");
}

#[test]
fn nodes_without_a_source_have_no_range() {
    let mut doc = parse("<r><a/></r>").expect("well-formed");
    let r = doc.root_element().expect("r");
    let a = doc.children(r)[0];
    doc.set_attribute(r, None, "k", "v").expect("attribute");
    let attr = doc.attribute_nodes(r)[0];
    assert_eq!(doc.range(attr), None, "attributes carry no range");
    let built = doc.append_element(r, None, "b").expect("append");
    assert_eq!(doc.range(built), None, "built nodes have no source");
    doc.remove(a).expect("remove");
    assert_eq!(doc.range(a), None, "a stale id has no range");
    let reused = doc.append_element(r, None, "c").expect("append");
    assert_eq!(
        doc.range(reused),
        None,
        "a recycled slot does not inherit the old range"
    );
    assert!(
        doc.range(r).is_some(),
        "an edited document keeps the ranges it parsed"
    );
}
