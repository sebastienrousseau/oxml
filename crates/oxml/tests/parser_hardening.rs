// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Regressions for the parsing core found by the 2026-10-06 audit.
//!
//! Each test here failed before its fix. They are black-box: only the
//! public entry points are used, so a refactor that keeps the
//! behaviour keeps them green.

use oxml::{Error, ErrorKind, Limits, parse, parse_bytes, parse_with};

/// A processing instruction whose target merely *starts* with `xml`
/// is not the XML declaration. Three sniffers used `strip_prefix("<?xml")`
/// without requiring whitespace after it, so `<?xml-stylesheet ...?>`
/// at offset 0 was read as a declaration and its attribute values as
/// pseudo-attributes.
mod declaration_sniffing {
    use super::*;

    #[test]
    fn a_stylesheet_pi_cannot_switch_the_document_to_xml_11() {
        // `&#x1;` is a legal reference only under 1.1 rules.
        let doc = r#"<?xml-stylesheet href="version='1.1'"?><a>&#x1;</a>"#;
        let err = parse(doc).expect_err("a 1.0 document rejects &#x1;");
        assert_eq!(err.kind, ErrorKind::IllegalCharacter('\u{1}'));
    }

    #[test]
    fn a_stylesheet_pi_cannot_declare_an_unsupported_version() {
        let doc = r#"<?xml-stylesheet href="version='2.0'"?><a/>"#;
        let parsed = parse(doc).expect("the PI is not a declaration");
        assert!(parsed.root_element().is_some());
    }

    #[test]
    fn a_stylesheet_pi_cannot_make_the_document_standalone() {
        // With `standalone="yes"` an entity declared in the external
        // subset may not be referenced; this document never said so.
        let doc = concat!(
            r#"<?xml-stylesheet href="standalone='yes'"?>"#,
            r#"<!DOCTYPE a SYSTEM "s.dtd"><a>&e;</a>"#
        );
        let parsed = parse(doc).expect("not standalone, so &e; is forgiven");
        assert!(parsed.root_element().is_some());
    }

    #[test]
    fn a_stylesheet_pi_cannot_change_the_encoding() {
        let bytes =
            "<?xml-stylesheet href=\"encoding='iso-8859-1'\"?><a>\u{e9}</a>"
                .as_bytes();
        let parsed = parse_bytes(bytes).expect("UTF-8 by default");
        let root = parsed.root_element().expect("root");
        assert_eq!(parsed.text(root), "\u{e9}");
    }

    #[test]
    fn a_real_declaration_still_works() {
        let doc = "<?xml version=\"1.1\" standalone=\"yes\"?><a>&#x1;</a>";
        let parsed = parse(doc).expect("1.1 accepts &#x1;");
        let root = parsed.root_element().expect("root");
        assert_eq!(parsed.text(root), "\u{1}");
        let bytes =
            b"<?xml version=\"1.0\" encoding=\"iso-8859-1\"?><a>\xe9</a>";
        let parsed = parse_bytes(bytes).expect("Latin-1 declared");
        let root = parsed.root_element().expect("root");
        assert_eq!(parsed.text(root), "\u{e9}");
    }
}

/// Namespace declarations are attributes, and `WFC: Unique Att Spec`
/// applies to them. They were skipped before the duplicate check ran.
mod duplicate_namespace_declarations {
    use super::*;

    #[test]
    fn a_prefix_declared_twice_on_one_element_is_rejected() {
        let err = parse(r#"<a xmlns:p="u1" xmlns:p="u2"/>"#)
            .expect_err("duplicate xmlns:p");
        assert_eq!(err.kind, ErrorKind::DuplicateAttribute("xmlns:p".into()));
    }

    #[test]
    fn the_default_namespace_declared_twice_is_rejected() {
        let err = parse(r#"<a xmlns="u1" xmlns="u2"/>"#)
            .expect_err("duplicate xmlns");
        assert_eq!(err.kind, ErrorKind::DuplicateAttribute("xmlns".into()));
    }

    #[test]
    fn the_same_prefix_on_nested_elements_is_fine() {
        let parsed = parse(r#"<a xmlns:p="u1"><b xmlns:p="u2"/></a>"#)
            .expect("shadowing is allowed");
        assert!(parsed.root_element().is_some());
    }
}

/// The duplicate-attribute check compared every new name against every
/// earlier one by string, so a wide element cost O(n²). It now scales
/// to the permissive ceiling.
mod wide_elements {
    use super::*;
    use std::fmt::Write as _;

    fn wide(attrs: usize) -> String {
        let mut doc = String::from("<a");
        for i in 0..attrs {
            write!(doc, " k{i}=\"v\"").expect("write to a String");
        }
        doc.push_str("/>");
        doc
    }

    #[test]
    fn a_duplicate_among_many_attributes_is_still_found() {
        let mut doc = wide(500);
        let _ = doc.pop();
        let _ = doc.pop();
        doc.push_str(" k7=\"again\"/>");
        let err = parse_with(&doc, Limits::permissive()).expect_err("k7 twice");
        assert_eq!(err.kind, ErrorKind::DuplicateAttribute("k7".into()));
    }

    #[test]
    fn a_namespaced_duplicate_among_many_is_found_by_expanded_name() {
        let mut doc = String::from(r#"<a xmlns:p="u" xmlns:q="u""#);
        for i in 0..300 {
            write!(doc, " k{i}=\"v\"").expect("write to a String");
        }
        doc.push_str(r#" p:x="1" q:x="2"/>"#);
        let err = parse(&doc).expect_err("p:x and q:x are the same attribute");
        assert!(matches!(err.kind, ErrorKind::DuplicateAttribute(_)));
    }

    #[test]
    fn the_permissive_ceiling_is_reachable() {
        let doc = wide(20_000);
        let parsed =
            parse_with(&doc, Limits::permissive()).expect("20,000 attributes");
        let root = parsed.root_element().expect("root");
        assert_eq!(parsed.attributes(root).len(), 20_000);
    }
}

/// Errors keep their offsets through all of the above.
#[test]
fn errors_are_still_positioned() {
    let err: Error = parse("<a><b></a>").expect_err("mismatched end tag");
    assert!(err.offset > 0);
}
