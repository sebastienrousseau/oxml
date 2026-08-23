// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Well-formedness rules the parser was not enforcing.
//!
//! Every one of these was a document the W3C suite says is not
//! well-formed and oxml accepted. They cluster into three causes, and
//! the first is worth naming because it produced four separate
//! defects: a check placed at the top of a scanning loop only ever sees
//! the *first* byte of a run, and the inner loop consumes the rest
//! without looking at it.

use oxml::{ErrorKind, parse};

/// A run scanner skipped past what the loop head was checking for.
mod inside_a_run {
    use super::{ErrorKind, parse};

    #[test]
    fn a_literal_cdata_end_is_rejected_anywhere_in_text() {
        // `CharData ::= [^<&]* - ([^<&]* ']]>' [^<&]*)`. It must be
        // written `]]&gt;`, so that a reader can always tell where a
        // CDATA section ends.
        for source in [
            "<doc>]]]></doc>", // starts at offset 1 of the run
            "<doc>abc]]]>def</doc>",
            "<doc>a]]>b</doc>",
            "<doc>x]]>",
        ] {
            let error = parse(source).expect_err(source);
            assert_eq!(error.kind, ErrorKind::IllegalCdataEnd, "{source}");
        }
    }

    #[test]
    fn cdata_sections_do_not_nest() {
        // The outer section ends at the *first* `]]>`, so what follows
        // is text containing a second one -- which is how the
        // specification makes nesting impossible rather than by a rule
        // against it.
        let source = "<doc>\n<![CDATA[\n<![CDATA[x]]>\n]]>\n</doc>";
        assert_eq!(
            parse(source).expect_err("nested").kind,
            ErrorKind::IllegalCdataEnd
        );
    }

    #[test]
    fn a_less_than_is_rejected_anywhere_in_an_attribute_value() {
        // `a="<x"` was rejected and `a="1 < 2"` was not, because only
        // the first byte of the run reached the check.
        for source in [
            r#"<doc a="<x"/>"#,
            r#"<doc a="1 < 2"/>"#,
            r#"<doc a="ends<"/>"#,
        ] {
            let error = parse(source).expect_err(source);
            assert_eq!(
                error.kind,
                ErrorKind::IllegalCharacter('<'),
                "{source}"
            );
        }
    }

    #[test]
    fn the_legal_spellings_still_parse() {
        // The point of the rules above is that these are how you write
        // those characters, so they must keep working.
        let doc = parse("<doc>a]]&gt;b</doc>").expect("escaped");
        assert_eq!(doc.text(doc.root()), "a]]>b");

        let doc = parse("<doc><![CDATA[a]]></doc>").expect("cdata");
        assert_eq!(doc.text(doc.root()), "a");

        let doc = parse(r#"<doc a="1 &lt; 2"/>"#).expect("escaped lt");
        let root = doc.root_element().expect("root");
        assert_eq!(doc.attribute(root, "a"), Some("1 < 2"));
    }
}

/// The declaration's own version string was the one field nothing
/// checked.
mod version_number {
    use super::{ErrorKind, parse};

    #[test]
    fn a_version_number_must_match_its_production() {
        // `VersionNum ::= '1.' [0-9]+`
        for source in [
            "<?xml version=\"1.0 \" ?>\n<doc></doc>", // trailing space
            "<?xml version=\"1.0?\"?>\n<doc/>",
            "<?xml version=\"1.0^\"?>\n<doc/>",
            "<?xml version=\"1.\"?><doc/>",
            "<?xml version=\"\"?><doc/>",
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn an_unrecognised_1_x_version_is_accepted_and_processed_as_1_0() {
        // XML 1.0 5th edition made this a *forwards-compatibility*
        // matter rather than an error: a processor that does not know
        // `1.2` should accept the document and process it as 1.0. So
        // this is not a gap -- it is the rule.
        assert!(parse("<?xml version=\"1.2\"?><doc/>").is_ok());
        assert!(parse("<?xml version=\"1.99\"?><doc/>").is_ok());
    }

    #[test]
    fn a_malformed_number_and_an_unsupported_one_differ() {
        // `2.0` is a version number this crate does not implement.
        // `1.0^` is not a version number at all. Reporting both the
        // same way would tell a caller nothing about which they have.
        assert_eq!(
            parse("<?xml version=\"1.0^\"?><doc/>")
                .expect_err("malformed")
                .kind,
            ErrorKind::MalformedDeclaration
        );
        assert_eq!(
            parse("<?xml version=\"2.0\"?><doc/>")
                .expect_err("unsupported")
                .kind,
            ErrorKind::UnsupportedVersion
        );
    }

    #[test]
    fn the_versions_this_crate_implements_still_parse() {
        assert!(parse("<?xml version=\"1.0\"?><doc/>").is_ok());
        assert!(parse("<?xml version=\"1.1\"?><doc/>").is_ok());
    }
}

/// Rules the Namespaces specification adds on top of XML's grammar.
mod namespaces {
    use super::parse;

    #[test]
    fn a_namespace_declaration_needs_a_prefix_to_declare() {
        // `PrefixedAttName ::= 'xmlns:' NCName`. `xmlns:` parses as a
        // name, so nothing in XML's own grammar rejects it.
        assert!(parse(r#"<foo xmlns:="urn:u" />"#).is_err());
    }

    #[test]
    fn undeclaring_a_prefix_is_xml_1_1_only() {
        // Binding a prefix to the empty string undeclares it, which
        // XML 1.1 added. In a 1.0 document it is an error -- and
        // treating it as a no-op silently changed which namespace the
        // element was in.
        let v10 = r#"<a:foo xmlns:a="urn:u" xmlns:a=""/>"#;
        assert!(parse(v10).is_err(), "1.0 must reject an undeclaration");
    }

    #[test]
    fn a_processing_instruction_target_has_no_colon() {
        // Namespaces narrows `PITarget` from `Name` to `NCName`. XML's
        // own grammar allows the colon, which is why this is easy to
        // miss.
        assert!(parse("<?a:b bogus?>\n<foo/>").is_err());
        // A target without one is still fine.
        assert!(parse("<?ab bogus?>\n<foo/>").is_ok());
    }
}

/// Constraints declared in the internal subset that were parsed and
/// then not enforced.
mod internal_subset {
    use super::{ErrorKind, parse};

    #[test]
    fn a_parameter_entity_reference_may_not_appear_inside_a_declaration() {
        // `WFC: PEs in Internal Subset`. A `%name;` may appear *where a
        // declaration can appear*, but not *inside* one. In the
        // external subset it is permitted, which is why this rule is
        // conditional rather than absolute.
        for source in [
            r#"<!DOCTYPE d [<!ENTITY % e ""><!ENTITY f "%e;">]><d/>"#,
            r#"<!DOCTYPE d [<!ENTITY % e1 ""><!ENTITY % e2 "%e1;">]><d/>"#,
        ] {
            assert!(parse(source).is_err(), "{source}");
        }
    }

    #[test]
    fn declaring_a_parameter_entity_does_not_switch_the_rule_off() {
        // This is how the rule came to be unenforced: declaring a
        // parameter entity set the same flag that means "declarations
        // from outside may have been pulled in", so the first
        // `<!ENTITY % … >` in a document disabled the check for every
        // declaration after it -- including the one that violates it.
        let source = r#"<!DOCTYPE d [
            <!ENTITY % harmless "text">
            <!ENTITY % second "more">
            <!ENTITY offender "%harmless;">
        ]><d/>"#;
        assert!(parse(source).is_err(), "the third declaration violates it");
    }

    #[test]
    fn an_external_entity_may_not_be_referenced_in_an_attribute_value() {
        // `WFC: No External Entity References`. Directly, and through
        // an internal entity whose text names it -- an indirect
        // reference is still a reference.
        let direct = r#"<!DOCTYPE r [<!ENTITY x SYSTEM "x.ent">]><r a="&x;"/>"#;
        let indirect = r#"<!DOCTYPE r [<!ENTITY x SYSTEM "x.ent"><!ENTITY i "&x;">]><r a="&i;"/>"#;
        for source in [direct, indirect] {
            let error = parse(source).expect_err(source);
            assert!(
                matches!(error.kind, ErrorKind::ForbiddenEntityReference(_)),
                "{source}: {:?}",
                error.kind
            );
        }
    }

    #[test]
    fn an_external_entity_in_content_is_still_permitted() {
        // The rule is about attribute values. In content the reference
        // is legal and expands to nothing, because nothing is ever
        // fetched -- which is the design, not a limitation being
        // papered over.
        let source = r#"<!DOCTYPE r [<!ENTITY x SYSTEM "x.ent">]><r>&x;</r>"#;
        let doc = parse(source).expect("legal in content");
        assert_eq!(doc.text(doc.root()), "");
    }

    #[test]
    fn an_unparsed_entity_may_not_be_referenced_anywhere() {
        // `WFC: Parsed Entity`. An `NDATA` entity is not text and has
        // no replacement, so a reference to one is meaningless in
        // content as well as in an attribute.
        let decl =
            r#"<!NOTATION J SYSTEM "J"><!ENTITY im SYSTEM "i.jpg" NDATA J>"#;
        for source in [
            &alloc_fmt(decl, r#"<r a="&im;"/>"#),
            &alloc_fmt(decl, r"<r>&im;</r>"),
        ] {
            let error = parse(source).expect_err(source);
            assert!(
                matches!(error.kind, ErrorKind::ForbiddenEntityReference(_)),
                "{source}: {:?}",
                error.kind
            );
        }
    }

    fn alloc_fmt(decl: &str, body: &str) -> String {
        format!("<!DOCTYPE r [{decl}]>{body}")
    }

    #[test]
    fn an_ordinary_internal_entity_is_unaffected() {
        let source = r#"<!DOCTYPE r [<!ENTITY g "hi">]><r a="&g;">&g;</r>"#;
        let doc = parse(source).expect("well-formed");
        let root = doc.root_element().expect("root");
        assert_eq!(doc.attribute(root, "a"), Some("hi"));
        assert_eq!(doc.text(root), "hi");
    }
}
