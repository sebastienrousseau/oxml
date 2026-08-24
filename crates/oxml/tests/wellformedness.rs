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

/// An `<!ATTLIST>` default value is an `AttValue`, and the constraints
/// on one apply at the point of *declaration* -- whether or not any
/// element ever uses the default.
///
/// None of them were checked, because the value was parsed with
/// `quoted()` and thrown away. Six W3C failures were this one omission.
mod attlist_defaults {
    use super::parse;

    /// `<!DOCTYPE d [ … ]><d></d>` around an internal subset.
    fn doc(subset: &str) -> String {
        format!("<!DOCTYPE d [{subset}]><d></d>")
    }

    #[test]
    fn an_entity_must_be_declared_before_it_is_used() {
        // `WFC: Entity Declared` -- the declaration must not come
        // *after* the reference. Declaring it later reads naturally and
        // is not allowed.
        assert!(
            parse(&doc(r#"<!ATTLIST d a CDATA "&e;"><!ENTITY e "v">"#))
                .is_err()
        );
        // The other way round is fine.
        assert!(
            parse(&doc(r#"<!ENTITY e "v"><!ATTLIST d a CDATA "&e;">"#)).is_ok()
        );
    }

    #[test]
    fn an_undeclared_entity_is_rejected() {
        assert!(parse(&doc(r#"<!ATTLIST d a CDATA "&foo;">"#)).is_err());
    }

    #[test]
    fn an_entity_may_not_refer_to_itself_however_indirectly() {
        // `WFC: No Recursion`. Three entities forming a cycle, reached
        // through a default value -- so the cycle is only visible if
        // the expansion is actually followed.
        let cycle =
            r#"<!ENTITY e1 "&e2;"><!ENTITY e2 "&e3;"><!ENTITY e3 "&e1;">"#;
        for default in [
            r#"<!ATTLIST d a CDATA "&e1;">"#,
            r#"<!ATTLIST d a CDATA #FIXED "&e1;">"#,
        ] {
            assert!(
                parse(&doc(&format!("{cycle}{default}"))).is_err(),
                "{default}"
            );
        }
    }

    #[test]
    fn nesting_without_a_cycle_is_fine() {
        // The recursion check must follow expansions without treating
        // depth as a cycle.
        let subset =
            r#"<!ENTITY x "1"><!ENTITY y "&x;2"><!ATTLIST d a CDATA "&y;">"#;
        assert!(parse(&doc(subset)).is_ok());
    }

    #[test]
    fn an_unparsed_or_external_entity_may_not_be_referenced() {
        let unparsed =
            r#"<!ENTITY e SYSTEM "nul" NDATA n><!ATTLIST d a CDATA "&e;">"#;
        let external =
            r#"<!ENTITY e SYSTEM "x.ent"><!ATTLIST d a CDATA "&e;">"#;
        assert!(parse(&doc(unparsed)).is_err(), "unparsed");
        assert!(parse(&doc(external)).is_err(), "external");
    }

    #[test]
    fn a_literal_less_than_is_rejected_here_too() {
        // `AttValue ::= '"' ([^<&"] | Reference)* '"'` -- the same
        // production, so the same rule.
        assert!(parse(&doc(r#"<!ATTLIST d a CDATA "1 < 2">"#)).is_err());
        assert!(parse(&doc(r#"<!ATTLIST d a CDATA "1 &lt; 2">"#)).is_ok());
    }

    #[test]
    fn ordinary_defaults_still_work() {
        for subset in [
            r#"<!ATTLIST d a CDATA "plain">"#,
            r#"<!ATTLIST d a CDATA "a &amp; b">"#,
            r#"<!ATTLIST d a CDATA "&#65;">"#,
            r"<!ATTLIST d a CDATA #IMPLIED>",
            r"<!ATTLIST d a CDATA #REQUIRED>",
        ] {
            assert!(parse(&doc(subset)).is_ok(), "{subset}");
        }
    }
}

/// A reference that is not a character reference names an entity, and
/// the name has to be one.
mod reference_names {
    use super::parse;

    #[test]
    fn a_reference_name_must_be_a_name() {
        // `&49;` has no `#`, so it is an entity reference -- and `49`
        // cannot start a name. Nothing checked this, so it read as a
        // reference to an entity that could not exist.
        let subset = r#"<!ELEMENT r EMPTY><!ENTITY a "bad: &49;">"#;
        assert!(parse(&format!("<!DOCTYPE r [{subset}]><r/>")).is_err());
    }

    #[test]
    fn the_legal_forms_are_unaffected() {
        for value in ["ok: &amp;", "ok: &#49;", "ok: &#x31;"] {
            let subset = format!(r#"<!ELEMENT r EMPTY><!ENTITY a "{value}">"#);
            assert!(
                parse(&format!("<!DOCTYPE r [{subset}]><r/>")).is_ok(),
                "{value}"
            );
        }
    }
}

/// `NDataDecl ::= S 'NDATA' S Name`, and only a *general* entity may
/// have one.
mod ndata {
    use super::parse;

    #[test]
    fn whitespace_before_ndata_is_part_of_the_production() {
        // Skipping whitespace before looking for the keyword accepted
        // `"f.eps"NDATA n`, which is not an entity with a notation --
        // it is malformed.
        let bad = r#"<!DOCTYPE d [<!NOTATION n SYSTEM "e"><!ENTITY f SYSTEM "f.eps"NDATA n>]><d></d>"#;
        let good = r#"<!DOCTYPE d [<!NOTATION n SYSTEM "e"><!ENTITY f SYSTEM "f.eps" NDATA n>]><d></d>"#;
        assert!(parse(bad).is_err(), "missing space");
        assert!(parse(good).is_ok(), "with space");
    }

    #[test]
    fn a_parameter_entity_may_not_be_unparsed() {
        // `PEDef ::= EntityValue | ExternalID` -- there is no
        // NDataDecl in it. Only a general entity can be unparsed.
        let bad = r#"<!DOCTYPE d [<!NOTATION J SYSTEM "J"><!ENTITY % p SYSTEM "i.jpg" NDATA J>]><d></d>"#;
        let good = r#"<!DOCTYPE d [<!ENTITY % p SYSTEM "p.ent">]><d></d>"#;
        assert!(parse(bad).is_err(), "parameter entity with NDATA");
        assert!(parse(good).is_ok(), "external parameter entity");
    }
}

/// `WFC: No < in Attribute Values`, including through an entity.
mod less_than_via_entity {
    use super::parse;

    #[test]
    fn an_entity_carrying_a_less_than_may_not_reach_an_attribute() {
        // A literal `<` is legal in an entity's *value* and illegal in
        // an attribute value, so the constraint is on where the entity
        // is referenced -- not on the declaration.
        let direct =
            r#"<!DOCTYPE r [<!ENTITY w "has <lt> inside">]><r a="&w;"/>"#;
        let indirect = r#"<!DOCTYPE r [<!ENTITY w "has <lt>"><!ENTITY i "&w;">]><r a="&i;"/>"#;
        assert!(parse(direct).is_err(), "direct");
        assert!(parse(indirect).is_err(), "indirect");
    }

    #[test]
    fn a_character_reference_is_still_permitted() {
        // `&#60;` stands for the character rather than introducing
        // markup, which is exactly why it is the way to write one.
        let doc = parse(r#"<r a="1 &#60; 2"/>"#).expect("character reference");
        let root = doc.root_element().expect("root");
        assert_eq!(doc.attribute(root, "a"), Some("1 < 2"));
    }

    #[test]
    fn the_same_entity_is_fine_in_content() {
        // Content may contain an entity whose text has a `<`; it is
        // only attribute values the rule covers. oxml substitutes it as
        // text rather than parsing it as markup -- see the note in
        // doc/CONFORMANCE.md.
        let source = r#"<!DOCTYPE r [<!ENTITY w "plain">]><r>&w;</r>"#;
        let doc = parse(source).expect("well-formed");
        assert_eq!(doc.text(doc.root()), "plain");
    }
}

/// Rules that live outside the DTD: the prolog, reserved namespaces,
/// and names the Namespaces specification narrows.
mod prolog_and_names {
    use super::parse;

    #[test]
    fn nothing_may_precede_the_xml_declaration() {
        // `prolog ::= XMLDecl? Misc*` -- the declaration comes first or
        // not at all. Skipping leading whitespace before looking for
        // `<?xml` accepted a document that begins with a blank line.
        assert!(parse("\r\n<?xml version=\"1.0\"?>\r\n<doc/>").is_err());
        assert!(parse(" <?xml version=\"1.0\"?><doc/>").is_err());
        // Without a declaration, leading whitespace is ordinary `Misc`.
        assert!(parse("\r\n<doc/>").is_ok());
        assert!(parse("<?xml version=\"1.0\"?><doc/>").is_ok());
    }

    #[test]
    fn the_reserved_namespaces_may_not_be_the_default() {
        // These were checked for *prefixed* declarations and not for
        // the default one, so the rule applied to `xmlns:a=` and not to
        // `xmlns=`.
        for uri in [
            "http://www.w3.org/XML/1998/namespace",
            "http://www.w3.org/2000/xmlns/",
        ] {
            let source = format!("<foo xmlns=\"{uri}\"/>");
            assert!(parse(&source).is_err(), "{uri}");
        }
        assert!(parse(r#"<foo xmlns="urn:ordinary"/>"#).is_ok());
    }

    #[test]
    fn entity_and_notation_names_have_no_colon() {
        // Namespaces narrows both from `Name` to `NCName`. XML's own
        // grammar allows the colon, which is why it needs saying.
        assert!(parse(r#"<!DOCTYPE f [<!ENTITY a:b "x">]><f/>"#).is_err());
        assert!(
            parse(r#"<!DOCTYPE f [<!NOTATION a:b SYSTEM "n">]><f/>"#).is_err()
        );
        assert!(parse(r#"<!DOCTYPE f [<!ENTITY ab "x">]><f/>"#).is_ok());
    }

    #[test]
    fn an_entity_named_inside_another_must_be_declared() {
        // Skipping the reference silently produced a document missing
        // the content it asked for, with nothing to say so.
        let source = r#"<!DOCTYPE d [<!ENTITY foo "&bar;">]><d a="&foo;"></d>"#;
        assert!(parse(source).is_err());
    }

    #[test]
    fn an_ampersand_inside_cdata_is_not_a_reference() {
        // This is how enforcing the rule above first broke a document
        // that was correct: the scan looked for `&` textually, and
        // `<![CDATA[&foo;]]>` inside an entity's replacement text is
        // text, not a reference to an undeclared entity.
        let source = r#"<!DOCTYPE d [<!ELEMENT d (#PCDATA)><!ENTITY e "<![CDATA[&foo;]]>">]><d>&e;</d>"#;
        assert!(parse(source).is_ok(), "CDATA in replacement text");
    }
}
