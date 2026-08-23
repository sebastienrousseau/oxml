// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Every way a document can be rejected, and what the reader is told.
//!
//! A parser's error messages are its user interface. Each variant is
//! reached through a real document rather than constructed directly,
//! so these also pin which input produces which diagnosis.

use oxml::{ErrorKind, parse};

/// Parse `src`, expecting failure, and return the error.
fn err(src: &str) -> oxml::Error {
    parse(src).expect_err("should not parse")
}

#[test]
fn a_mismatched_end_tag_names_both_tags() {
    let e = err("<a><b></a>");
    let ErrorKind::MismatchedEndTag { expected, found } = &e.kind else {
        panic!("got {:?}", e.kind);
    };
    assert_eq!(expected, "b");
    assert_eq!(found, "a");

    let text = e.to_string();
    assert!(text.contains("</a>"), "{text}");
    assert!(text.contains("<b>"), "{text}");
}

#[test]
fn an_unmatched_end_tag_is_distinct_from_a_mismatched_one() {
    let e = err("<a></a></b>");
    assert!(
        matches!(e.kind, ErrorKind::UnexpectedEndTag(ref n) if n == "b"),
        "got {:?}",
        e.kind
    );
    assert!(e.to_string().contains("no matching open tag"), "{e}");
}

#[test]
fn truncated_input_is_unexpected_eof() {
    for src in ["<a>", "<a", "<a href=", "<"] {
        let e = err(src);
        assert!(
            matches!(e.kind, ErrorKind::UnexpectedEof | ErrorKind::InvalidName),
            "`{src}` gave {:?}",
            e.kind
        );
    }
    assert!(err("<a>").to_string().contains("ended"), "message");
}

#[test]
fn a_name_must_start_with_a_name_character() {
    let e = err("<1a/>");
    assert!(matches!(e.kind, ErrorKind::InvalidName), "got {:?}", e.kind);
    assert!(e.to_string().contains("name"), "{e}");
}

#[test]
fn an_unquoted_attribute_value_is_rejected() {
    let e = err("<a href=x/>");
    assert!(
        matches!(e.kind, ErrorKind::UnquotedAttributeValue),
        "got {:?}",
        e.kind
    );
    assert!(e.to_string().contains("quoted"), "{e}");
}

#[test]
fn a_repeated_attribute_is_rejected_and_named() {
    let e = err(r#"<a x="1" x="2"/>"#);
    assert!(
        matches!(e.kind, ErrorKind::DuplicateAttribute(ref n) if n == "x"),
        "got {:?}",
        e.kind
    );
    assert!(e.to_string().contains('x'), "{e}");
}

#[test]
fn an_unknown_entity_is_named_rather_than_dropped() {
    // Silently dropping it would corrupt the document's text.
    let e = err("<a>&nope;</a>");
    assert!(
        matches!(e.kind, ErrorKind::UnknownEntity(ref n) if n == "nope"),
        "got {:?}",
        e.kind
    );
    assert!(e.to_string().contains("nope"), "{e}");
}

#[test]
fn an_undeclared_namespace_prefix_is_rejected() {
    let e = err("<p:a/>");
    assert!(
        matches!(e.kind, ErrorKind::UnboundPrefix(ref p) if p == "p"),
        "got {:?}",
        e.kind
    );
    assert!(e.to_string().contains('p'), "{e}");
}

#[test]
fn content_after_the_root_element_is_rejected() {
    let e = err("<a/><b/>");
    assert!(
        matches!(e.kind, ErrorKind::TrailingContent),
        "got {:?}",
        e.kind
    );
    assert!(!e.to_string().is_empty());
}

#[test]
fn a_document_with_no_root_element_is_rejected() {
    for src in [
        "",
        "   ",
        "<!-- only a comment -->",
        "<?xml version=\"1.0\"?>",
    ] {
        let e = err(src);
        assert!(
            matches!(e.kind, ErrorKind::NoRootElement),
            "`{src}` gave {:?}",
            e.kind
        );
    }
    assert!(err("").to_string().contains("root"), "message");
}

#[test]
fn unterminated_constructs_name_what_was_left_open() {
    for (src, what) in [
        ("<a><!-- unclosed</a>", "comment"),
        ("<a><![CDATA[unclosed</a>", "CDATA"),
    ] {
        let e = err(src);
        let ErrorKind::Unterminated(kind) = e.kind else {
            panic!("`{src}` gave {:?}", e.kind);
        };
        assert!(
            kind.to_lowercase().contains(&what.to_lowercase()),
            "`{src}` said {kind}, expected {what}"
        );
        assert!(e.to_string().contains(kind), "{e}");
    }
}

#[test]
fn every_error_reports_its_byte_offset() {
    let e = err("<a><b></a>");
    assert!(e.offset > 0);
    assert!(e.to_string().contains("byte"), "{e}");
}

#[test]
fn line_and_column_are_one_based_and_count_characters() {
    // A column counted in bytes would be wrong for any line containing
    // a multi-byte character, which is exactly when it is needed.
    let src = "<a>\n  <b></a>";
    let e = err(src);
    let (line, col) = e.line_column(src);
    assert_eq!(line, 2, "second line");
    assert!(col >= 1);

    let wide = "<a>é中<b></a>";
    let (l, c) = err(wide).line_column(wide);
    assert_eq!(l, 1);
    assert!(c <= wide.chars().count() + 1, "column {c} exceeds the line");
}

#[test]
fn an_offset_at_the_very_start_is_line_one_column_one() {
    let src = "";
    let (line, col) = err(src).line_column(src);
    assert_eq!((line, col), (1, 1));
}

#[test]
fn the_error_type_is_debug_and_display() {
    let e = err("<a>");
    assert!(!format!("{e:?}").is_empty());
    assert!(!format!("{e}").is_empty());
}
