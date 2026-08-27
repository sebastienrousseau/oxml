// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! An entity's replacement text is markup, not characters.
//!
//! XML 1.0 section 4.4.2: a general entity referenced from content is
//! *included*, meaning its replacement text is parsed as content. A
//! parser that substitutes it as characters accepts documents that are
//! not well-formed, and this file is the thirteen the W3C suite has.
//!
//! The distinction that makes it subtle is in section 4.4 and is
//! tested at the bottom: inside an entity's declaration, character
//! references are *included* and general entity references are
//! *bypassed*. `<!ENTITY e "&#38;">` therefore has the single
//! character `&` as its replacement text -- markup, starting no
//! reference, so an error -- while `<!ENTITY e "&amp;">` still holds a
//! reference and is perfectly well-formed.

use oxml::{ErrorKind, parse};

/// Documents the W3C suite marks not-well-formed, all for one reason.
#[test]
fn replacement_text_that_is_not_content_is_refused() {
    let cases: &[(&str, &str)] = &[
        (
            "closes an element it did not open",
            r#"<!DOCTYPE doc [<!ENTITY e "</foo><foo>">]><doc><foo>&e;</foo></doc>"#,
        ),
        (
            "opens an element it does not close",
            r#"<!DOCTYPE doc [<!ENTITY e "&#60;foo>">]><doc>&e;</doc>"#,
        ),
        (
            "a `<` in an attribute value",
            r#"<!DOCTYPE doc [<!ENTITY e "<foo a='&#60;'></foo>">]><doc>&e;</doc>"#,
        ),
        (
            "an `&` in an attribute value starting no reference",
            r#"<!DOCTYPE doc [<!ENTITY e "<foo a='&#38;'></foo>">]><doc>&e;</doc>"#,
        ),
        (
            "a bare `&` in content",
            r#"<!DOCTYPE doc [<!ENTITY e "&#38;">]><doc>&e;</doc>"#,
        ),
        (
            "a bare `&` in an attribute value",
            r#"<!DOCTYPE doc [<!ENTITY e "&#38;">]><doc a="&e;"></doc>"#,
        ),
        (
            "a reference assembled across the entity boundary",
            r#"<!DOCTYPE doc [<!ENTITY e "&#38;">]><doc>&e;#97;</doc>"#,
        ),
        (
            "a character reference assembled across the boundary",
            r#"<!DOCTYPE doc [<!ENTITY e "&#38;#9">]><doc>&e;7;</doc>"#,
        ),
        (
            "a reserved processing-instruction target",
            r#"<!DOCTYPE doc [<!ENTITY e "<?xml encoding='UTF-8'?>">]><doc>&e;</doc>"#,
        ),
        (
            "a comment the entity opens and does not close",
            r#"<!DOCTYPE doc [<!ENTITY e "&#60;!--">]><doc>&e;--></doc>"#,
        ),
    ];
    for (why, doc) in cases {
        assert!(
            parse(doc).is_err(),
            "accepted a document that is not well-formed: {why}\n  {doc}"
        );
    }
}

/// The two that look identical and are not.
///
/// This is the whole reason the check cannot simply reject a `&` in
/// replacement text.
#[test]
fn a_bypassed_reference_is_not_an_included_character() {
    // `&#38;` is *included* when the entity is declared, so the
    // replacement text is a bare `&`: markup, starting nothing.
    let included = r#"<!DOCTYPE doc [<!ENTITY e "&#38;">]><doc>&e;</doc>"#;
    assert!(parse(included).is_err(), "a bare `&` is not content");

    // `&amp;` is *bypassed*, so the replacement text still holds a
    // reference, which resolves where it is used.
    let bypassed = r#"<!DOCTYPE doc [<!ENTITY e "&amp;">]><doc>&e;</doc>"#;
    let doc = parse(bypassed).expect("a reference resolves");
    assert_eq!(doc.text(doc.root()), "&");
}

/// Character references and predefined entities are not re-parsed.
///
/// Only a *declared* entity's replacement text is included as markup.
/// Were that not so, every `&#38;` in every document would fail.
#[test]
fn direct_references_are_characters_not_markup() {
    // Content, and the character each must yield.
    for (doc, expected) in [
        ("<doc>&#38;</doc>", "&"),
        ("<doc>&amp;</doc>", "&"),
        ("<doc>&#60;</doc>", "<"),
        ("<doc>a &lt; b</doc>", "a < b"),
    ] {
        let parsed = parse(doc).unwrap_or_else(|e| {
            panic!("{doc:?} is well-formed but was refused: {e}")
        });
        assert_eq!(parsed.text(parsed.root()), expected, "{doc:?}");
    }

    // The same in an attribute value. An attribute contributes nothing
    // to its element's string-value, so this reads the value itself
    // rather than the text -- checking `text()` here passed for the
    // wrong reason and then failed for the wrong reason.
    for (doc, expected) in [
        ("<doc a=\"&#38;\"></doc>", "&"),
        ("<doc a=\"&amp;\"></doc>", "&"),
        ("<doc a=\"&#60;\"></doc>", "<"),
    ] {
        let parsed = parse(doc).unwrap_or_else(|e| {
            panic!("{doc:?} is well-formed but was refused: {e}")
        });
        let root = parsed.root_element().expect("a root element");
        assert_eq!(parsed.attribute(root, "a"), Some(expected), "{doc:?}");
    }
}

/// Replacement text that *is* well-formed content stays accepted.
#[test]
fn well_formed_replacement_text_is_accepted() {
    for doc in [
        r#"<!DOCTYPE doc [<!ENTITY e "<b/>">]><doc>&e;</doc>"#,
        r#"<!DOCTYPE doc [<!ENTITY e "<b>text</b>">]><doc>&e;</doc>"#,
        r#"<!DOCTYPE doc [<!ENTITY e "plain text">]><doc>&e;</doc>"#,
        r#"<!DOCTYPE doc [<!ENTITY e "<b a='1'/>">]><doc>&e;</doc>"#,
        r#"<!DOCTYPE doc [<!ENTITY e "<!-- a comment -->">]><doc>&e;</doc>"#,
        r#"<!DOCTYPE doc [<!ENTITY e "<![CDATA[<not markup>]]>">]><doc>&e;</doc>"#,
        // Nested: one entity referencing another.
        r#"<!DOCTYPE doc [<!ENTITY a "<b/>"><!ENTITY e "&a;">]><doc>&e;</doc>"#,
    ] {
        let parsed = parse(doc).unwrap_or_else(|e| {
            panic!("{doc:?} is well-formed but was refused: {e}")
        });
        assert!(parsed.root_element().is_some(), "{doc:?} has a root");
    }
}

/// An error inside replacement text is reported at the reference.
///
/// An offset into text that does not appear in the document would send
/// a caller's caret somewhere meaningless.
#[test]
fn the_error_points_at_the_reference_not_inside_the_entity() {
    let doc = r#"<!DOCTYPE doc [<!ENTITY e "&#38;">]><doc>&e;</doc>"#;
    let error = parse(doc).expect_err("not well-formed");
    let at = doc.find("&e;").expect("the reference is in the document");
    assert_eq!(
        error.offset, at,
        "offset {} should point at the reference at {at}",
        error.offset
    );
    assert!(
        error.offset < doc.len(),
        "offset must be inside the document"
    );
}

/// A self-referential entity terminates rather than recursing forever.
#[test]
fn a_recursive_entity_is_bounded() {
    let doc = r#"<!DOCTYPE doc [<!ENTITY e "<b>&e;</b>">]><doc>&e;</doc>"#;
    let error = parse(doc).expect_err("must not recurse without bound");
    assert!(
        matches!(
            error.kind,
            ErrorKind::EntityLimitExceeded | ErrorKind::UnknownEntity(_)
        ),
        "unexpected: {:?}",
        error.kind
    );
}
