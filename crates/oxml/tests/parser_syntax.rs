// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! XML syntax the parser must accept, and the truncations it must
//! reject.
//!
//! Each construct here is one a real document uses — declarations,
//! doctypes, CDATA, processing instructions, entities, namespaces —
//! paired with the truncated form that must be an error rather than a
//! silently shortened document.

use oxml::{ErrorKind, NodeKind, XPath, parse};

fn text_of(src: &str, expr: &str) -> String {
    let doc = parse(src).expect("well-formed");
    XPath::compile(expr)
        .expect("valid")
        .evaluate(&doc)
        .to_str(&doc)
}

#[test]
fn an_xml_declaration_is_accepted_and_not_part_of_the_tree() {
    let doc = parse("<?xml version=\"1.0\" encoding=\"UTF-8\"?><a>x</a>")
        .expect("well-formed");
    let root = doc.root_element().expect("root element");
    assert_eq!(doc.element_name(root).map(|n| n.local.as_str()), Some("a"));
    assert_eq!(doc.text(root), "x");
}

#[test]
fn a_truncated_xml_declaration_is_rejected() {
    let e = parse("<?xml version=\"1.0\"").expect_err("truncated");
    assert!(
        matches!(e.kind, ErrorKind::Unterminated(w) if w.contains("XML")),
        "got {:?}",
        e.kind
    );
}

#[test]
fn a_doctype_is_skipped() {
    let doc = parse("<!DOCTYPE note SYSTEM \"note.dtd\"><note>x</note>")
        .expect("well-formed");
    assert_eq!(doc.text(doc.root_element().expect("root")), "x");
}

#[test]
fn a_truncated_doctype_is_rejected() {
    let e = parse("<!DOCTYPE note").expect_err("truncated");
    assert!(
        matches!(e.kind, ErrorKind::Unterminated(w) if w.contains("doctype")),
        "got {:?}",
        e.kind
    );
}

#[test]
fn a_processing_instruction_becomes_a_node() {
    let doc = parse("<?php echo 1; ?><a/>").expect("well-formed");
    let found = doc.descendants().any(|n| {
        matches!(
            doc.kind(n),
            Some(NodeKind::ProcessingInstruction { target, .. })
                if target == "php"
        )
    });
    assert!(found, "the processing instruction was dropped");
}

#[test]
fn cdata_content_is_text_and_is_not_re_parsed() {
    // The whole point of CDATA: markup inside it is data.
    let src = "<a><![CDATA[<b> & </b>]]></a>";
    let doc = parse(src).expect("well-formed");
    let root = doc.root_element().expect("root");
    assert_eq!(doc.text(root), "<b> & </b>");
    assert_eq!(doc.children(root).len(), 1, "one text node, no elements");
}

#[test]
fn an_unterminated_cdata_section_is_rejected() {
    let e = parse("<a><![CDATA[oops</a>").expect_err("truncated");
    assert!(
        matches!(e.kind, ErrorKind::Unterminated(w) if w.contains("CDATA")),
        "got {:?}",
        e.kind
    );
}

#[test]
fn the_predefined_entities_are_resolved() {
    assert_eq!(text_of("<a>&lt;&gt;&amp;&quot;&apos;</a>", "/a"), "<>&\"'");
}

#[test]
fn numeric_character_references_are_resolved() {
    assert_eq!(text_of("<a>&#65;&#x42;</a>", "/a"), "AB");
    assert_eq!(text_of("<a>&#233;&#x4e2d;</a>", "/a"), "é中");
    assert_eq!(text_of("<a>&#128512;</a>", "/a"), "😀", "beyond the BMP");
}

#[test]
fn entities_are_resolved_in_attribute_values_too() {
    assert_eq!(text_of("<a t=\"&lt;&amp;&#65;\"/>", "/a/@t"), "<&A");
}

#[test]
fn an_unterminated_start_tag_is_rejected() {
    for src in ["<a", "<a href=\"x\"", "<a "] {
        let e = parse(src).expect_err("truncated");
        assert!(
            matches!(
                e.kind,
                ErrorKind::Unterminated(_) | ErrorKind::UnexpectedEof
            ),
            "`{src}` gave {:?}",
            e.kind
        );
    }
}

#[test]
fn an_unterminated_end_tag_is_rejected() {
    let e = parse("<a></a").expect_err("truncated");
    assert!(
        matches!(
            e.kind,
            ErrorKind::Unterminated(_) | ErrorKind::UnexpectedEof
        ),
        "got {:?}",
        e.kind
    );
}

#[test]
fn both_quote_styles_are_accepted_for_attributes() {
    assert_eq!(text_of("<a t='single'/>", "/a/@t"), "single");
    assert_eq!(text_of("<a t=\"double\"/>", "/a/@t"), "double");
    assert_eq!(
        text_of("<a t='has \"double\" inside'/>", "/a/@t"),
        "has \"double\" inside"
    );
}

#[test]
fn a_default_namespace_applies_to_unprefixed_descendants() {
    let doc = parse("<r xmlns=\"urn:d\"><c/></r>").expect("well-formed");
    for local in ["r", "c"] {
        let id = doc
            .descendants()
            .find(|n| doc.element_name(*n).is_some_and(|e| e.local == local))
            .expect("element");
        assert_eq!(
            doc.element_name(id).and_then(|n| n.namespace.as_deref()),
            Some("urn:d"),
            "{local}"
        );
    }
}

#[test]
fn a_namespace_scope_ends_with_its_element() {
    // The prefix is declared on `inner` only; using it outside must
    // fail rather than silently resolving.
    assert!(parse("<r><i xmlns:p=\"urn:p\"><p:x/></i></r>").is_ok());
    assert!(parse("<r><i xmlns:p=\"urn:p\"/><p:x/></r>").is_err());
}

#[test]
fn whitespace_only_documents_and_trailing_whitespace_are_handled() {
    assert!(parse("  <a/>  ").is_ok(), "surrounding whitespace is legal");
    assert!(parse("  ").is_err(), "whitespace alone has no root element");
}

#[test]
fn comments_are_preserved_but_are_not_text() {
    let doc = parse("<a><!-- note -->x</a>").expect("well-formed");
    let root = doc.root_element().expect("root");
    assert_eq!(doc.text(root), "x");
    assert!(
        doc.children(root)
            .iter()
            .any(|c| matches!(doc.kind(*c), Some(NodeKind::Comment(_))))
    );
}

#[test]
fn nesting_up_to_the_limit_parses() {
    let src = format!(
        "{}{}",
        "<a>".repeat(oxml::MAX_DEPTH),
        "</a>".repeat(oxml::MAX_DEPTH)
    );
    let doc = parse(&src).expect("at the limit");
    assert!(doc.len() >= oxml::MAX_DEPTH);
}

#[test]
fn nesting_past_the_limit_is_an_error_not_a_stack_overflow() {
    // Parsing descends one frame per open element. Without a limit a
    // hostile document aborts the process, which no caller can catch —
    // and every front end of this crate reads documents it did not
    // write.
    for depth in [oxml::MAX_DEPTH + 1, 10_000, 100_000] {
        let src = format!("{}{}", "<a>".repeat(depth), "</a>".repeat(depth));
        let e = parse(&src).expect_err("beyond the limit");
        assert!(
            matches!(e.kind, ErrorKind::DepthLimitExceeded),
            "depth {depth} gave {:?}",
            e.kind
        );
        // The message names the bound that was hit, not its value:
        // the value is now caller-configurable via `Limits`.
        assert!(e.to_string().contains("depth limit"), "{e}");
    }
}

#[test]
fn an_unclosed_deep_document_is_also_bounded() {
    // No closing tags at all: the limit must apply on the way down,
    // not only when the document is balanced.
    let src = "<a>".repeat(100_000);
    assert!(parse(&src).is_err());
}

#[test]
fn an_unquoted_attribute_value_is_rejected_in_every_position() {
    for src in ["<a x=1/>", "<a x=y z=\"1\"/>", "<a x= />"] {
        let e = parse(src).expect_err("unquoted");
        assert!(
            matches!(
                e.kind,
                ErrorKind::UnquotedAttributeValue | ErrorKind::InvalidName
            ),
            "`{src}` gave {:?}",
            e.kind
        );
    }
}

#[test]
fn an_unterminated_attribute_value_is_rejected() {
    let e = parse("<a x=\"unclosed").expect_err("truncated");
    assert!(
        matches!(
            e.kind,
            ErrorKind::Unterminated(_) | ErrorKind::UnexpectedEof
        ),
        "got {:?}",
        e.kind
    );
}

#[test]
fn trailing_content_after_the_root_is_rejected_in_each_form() {
    for src in ["<a/>text", "<a/><b/>", "<a/><!-- c --><b/>"] {
        assert!(parse(src).is_err(), "`{src}` should not parse");
    }
    // A comment or whitespace alone after the root is legal.
    assert!(parse("<a/><!-- trailing comment -->").is_ok());
    assert!(parse("<a/>\n").is_ok());
}

#[test]
fn names_may_use_the_full_xml_name_range() {
    // Accented and non-Latin names are valid XML; rejecting them would
    // break every non-English document.
    for src in ["<é/>", "<中文/>", "<a.b/>", "<a-b/>", "<_a/>", "<a1/>"] {
        assert!(parse(src).is_ok(), "`{src}` should parse");
    }
    for src in ["<1a/>", "<-a/>", "<.a/>", "< a/>"] {
        assert!(parse(src).is_err(), "`{src}` should not parse");
    }
}

#[test]
fn an_unterminated_comment_is_rejected() {
    let e = parse("<a><!-- unclosed").expect_err("truncated");
    assert!(
        matches!(e.kind, ErrorKind::Unterminated(w) if w.contains("comment")),
        "got {:?}",
        e.kind
    );
}
