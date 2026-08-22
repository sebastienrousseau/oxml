// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Parser behaviour and well-formedness rules.

use oxml::{ErrorKind, NodeKind, parse};

#[test]
fn parses_a_minimal_document() {
    let d = parse("<a/>").expect("parses");
    let root = d.root_element().expect("has a root element");
    assert_eq!(d.element_name(root).unwrap().local, "a");
    assert!(d.children(root).is_empty());
}

#[test]
fn merges_adjacent_character_data() {
    // Text, entity, and CDATA in sequence must arrive as one text
    // node — a caller should never have to stitch runs together.
    let d = parse("<a>one &amp; <![CDATA[two]]> three</a>").expect("parses");
    let root = d.root_element().unwrap();
    let kids = d.children(root);
    assert_eq!(kids.len(), 1, "expected a single merged text node");
    assert_eq!(d.text(root), "one & two three");
}

#[test]
fn resolves_the_five_predefined_entities() {
    let d = parse("<a>&lt;&gt;&amp;&apos;&quot;</a>").expect("parses");
    assert_eq!(d.text(d.root_element().unwrap()), "<>&'\"");
}

#[test]
fn resolves_numeric_character_references() {
    let d = parse("<a>&#65;&#x42;</a>").expect("parses");
    assert_eq!(d.text(d.root_element().unwrap()), "AB");
}

/// External and custom entities are never expanded.
///
/// This is the XXE and billion-laughs surface. The parser rejects the
/// reference rather than resolving it, so a document cannot be made to
/// read a file or explode in memory through an entity.
#[test]
fn refuses_to_expand_custom_entities() {
    let src = r#"<!DOCTYPE a [<!ENTITY xxe SYSTEM "file:///etc/passwd">]>
                 <a>&xxe;</a>"#;
    let err = parse(src).expect_err("must not expand");
    assert!(matches!(err.kind, ErrorKind::UnknownEntity(_)));
}

#[test]
fn billion_laughs_does_not_expand() {
    let src = r#"<!DOCTYPE lolz [
        <!ENTITY lol "lol">
        <!ENTITY lol2 "&lol;&lol;&lol;&lol;&lol;">
    ]><lolz>&lol2;</lolz>"#;
    let err = parse(src).expect_err("must not expand");
    assert!(matches!(err.kind, ErrorKind::UnknownEntity(_)));
}

#[test]
fn namespaces_resolve_by_uri_not_prefix() {
    // Different prefixes, same URI: the names must be equal.
    let d = parse(r#"<r xmlns:a="urn:x" xmlns:b="urn:x"><a:e/><b:e/></r>"#)
        .expect("parses");
    let root = d.root_element().unwrap();
    let kids = d.children(root);
    let first = d.element_name(kids[0]).unwrap();
    let second = d.element_name(kids[1]).unwrap();
    assert_eq!(first, second);
    assert_eq!(first.namespace.as_deref(), Some("urn:x"));
}

/// An unprefixed attribute is in *no* namespace, even when its element
/// is in one. Conflating the two is the classic namespace bug.
#[test]
fn default_namespace_applies_to_elements_not_attributes() {
    let d = parse(r#"<r xmlns="urn:d" id="7"/>"#).expect("parses");
    let root = d.root_element().unwrap();
    assert_eq!(
        d.element_name(root).unwrap().namespace.as_deref(),
        Some("urn:d")
    );
    let attrs = d.attributes(root);
    assert_eq!(attrs.len(), 1);
    assert_eq!(attrs[0].name.namespace, None);
}

#[test]
fn undeclared_prefixes_are_rejected() {
    let err = parse("<a:e/>").expect_err("prefix is not bound");
    assert!(matches!(err.kind, ErrorKind::UnboundPrefix(_)));
}

#[test]
fn the_xml_prefix_is_bound_without_declaration() {
    let d = parse(r#"<a xml:lang="en"/>"#).expect("parses");
    let root = d.root_element().unwrap();
    assert_eq!(
        d.attributes(root)[0].name.namespace.as_deref(),
        Some("http://www.w3.org/XML/1998/namespace")
    );
}

#[test]
fn mismatched_end_tags_are_rejected() {
    let err = parse("<a></b>").expect_err("mismatched");
    assert!(matches!(err.kind, ErrorKind::MismatchedEndTag { .. }));
}

#[test]
fn duplicate_attributes_are_rejected() {
    let err = parse(r#"<a x="1" x="2"/>"#).expect_err("duplicate");
    assert!(matches!(err.kind, ErrorKind::DuplicateAttribute(_)));
}

#[test]
fn content_after_the_root_element_is_rejected() {
    let err = parse("<a/><b/>").expect_err("two roots");
    assert!(matches!(err.kind, ErrorKind::TrailingContent));
}

#[test]
fn a_document_needs_a_root_element() {
    let err = parse("<!-- just a comment -->").expect_err("no root element");
    assert!(matches!(err.kind, ErrorKind::NoRootElement));
}

#[test]
fn comments_and_processing_instructions_are_preserved() {
    let d = parse("<?xml version=\"1.0\"?><a><!--c--><?pi data?></a>")
        .expect("parses");
    let root = d.root_element().unwrap();
    let kids = d.children(root);
    assert!(matches!(d.kind(kids[0]), Some(NodeKind::Comment(c)) if c == "c"));
    assert!(matches!(
        d.kind(kids[1]),
        Some(NodeKind::ProcessingInstruction { target, data })
            if target == "pi" && data == "data"
    ));
}

#[test]
fn a_doctype_with_a_bracketed_subset_does_not_end_early() {
    // The `>` inside the internal subset must not terminate the
    // doctype.
    let d =
        parse("<!DOCTYPE a [<!ELEMENT a (#PCDATA)>]><a>x</a>").expect("parses");
    assert_eq!(d.text(d.root_element().unwrap()), "x");
}

#[test]
fn errors_report_a_usable_line_and_column() {
    let src = "<a>\n  <b>\n</a>";
    let err = parse(src).expect_err("mismatched");
    let (line, col) = err.line_column(src);
    assert!(line >= 2, "line was {line}");
    assert!(col >= 1);
}

#[test]
fn unterminated_constructs_are_rejected_not_hung() {
    for src in [
        "<a>",
        "<a><!-- unterminated",
        "<a attr='unterminated>",
        "<?pi unterminated",
    ] {
        assert!(parse(src).is_err(), "should reject: {src}");
    }
}
