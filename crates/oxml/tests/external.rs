// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Content the caller supplies for external entities and subsets.
//!
//! oxml performs no I/O in any of these. Every byte of external content
//! is handed to it by the test, which is exactly the shape the feature
//! has in a real program.

use oxml::{Limits, parse, parse_with_external};

/// Parse `doc` with `parts` available as external content.
fn with(
    doc: &str,
    parts: &[(&str, &str)],
) -> Result<oxml::Document, oxml::Error> {
    parse_with_external(doc, Limits::default(), &parts)
}

#[test]
fn without_a_source_an_external_reference_expands_to_nothing() {
    // The default, and the reason XXE is foreclosed by construction:
    // there is nothing to configure, because there is nothing to fetch.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    let parsed = parse(doc).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "");
}

#[test]
fn with_a_source_the_content_is_used() {
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    let parsed = with(doc, &[("g.ent", "hello")]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "hello");
}

#[test]
fn a_text_declaration_is_stripped_rather_than_inserted() {
    // It describes the entity; it is not part of it.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    let ent = r#"<?xml version="1.0" encoding="UTF-8"?>hello"#;
    let parsed = with(doc, &[("g.ent", ent)]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "hello");
}

#[test]
fn a_text_declaration_must_name_an_encoding() {
    // `TextDecl ::= '<?xml' VersionInfo? EncodingDecl S? '?>'` --
    // unlike a document's declaration, where it is optional.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    assert!(with(doc, &[("g.ent", r#"<?xml version="1.0"?>x"#)]).is_err());
    // And the version is optional here, where it is required there.
    let ok = with(doc, &[("g.ent", r#"<?xml encoding="UTF-8"?>x"#)]);
    assert!(ok.is_ok(), "version is optional in a text declaration");
}

#[test]
fn a_text_declaration_may_not_be_standalone() {
    // Only a document may say `standalone`.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    let ent = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>x"#;
    assert!(with(doc, &[("g.ent", ent)]).is_err());
}

#[test]
fn an_unreferenced_entity_is_never_read() {
    // A processor need not fetch what a document does not use, so an
    // unused entity's content is not checked -- even if it would fail
    // the rules above. Validating eagerly rejected valid documents.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>text</d>"#;
    let broken = r#"<?xml version="1.0"?>"#; // no encoding
    let parsed = with(doc, &[("g.ent", broken)]).expect("never read");
    assert_eq!(parsed.text(parsed.root()), "text");
}

#[test]
fn an_external_subsets_declarations_are_used() {
    // With the subset available, an entity declared only there resolves
    // -- which is the point of supplying it.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d>&g;</d>"#;
    let dtd = r#"<!ELEMENT d (#PCDATA)><!ENTITY g "from the subset">"#;
    let parsed = with(doc, &[("d.dtd", dtd)]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "from the subset");
}

#[test]
fn the_internal_subset_wins() {
    // "The first declaration binds", and the internal subset is read
    // first, so it takes precedence over the external one.
    let doc =
        r#"<!DOCTYPE d SYSTEM "d.dtd" [<!ENTITY g "internal">]><d>&g;</d>"#;
    let dtd = r#"<!ENTITY g "external">"#;
    let parsed = with(doc, &[("d.dtd", dtd)]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "internal");
}

#[test]
fn a_declaration_needing_a_parameter_entity_is_skipped_not_rejected() {
    // Parameter entities are not expanded, and in the external subset
    // `<!ELEMENT x (a,%choice;,c)>` is legal and common. Treating it as
    // malformed rejected valid documents outright.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d/>"#;
    let dtd = r#"
        <!ENTITY % choice "(a|b)">
        <!ELEMENT d (a,%choice;,c)>
        <!ENTITY g "still parsed">
    "#;
    assert!(with(doc, &[("d.dtd", dtd)]).is_ok());
}

#[test]
fn a_declaration_closed_by_a_parameter_entity_is_tolerated() {
    // `<!ENTITY % e ">">` then `<!ELEMENT doc (#PCDATA) %e;` -- the
    // declaration's own `>` comes from the entity, so scanning for one
    // runs to the end of the subset. That is expected here, not
    // malformed.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d></d>"#;
    let dtd = "<!ENTITY % e \">\">\n<!ELEMENT d (#PCDATA) %e;\n";
    assert!(with(doc, &[("d.dtd", dtd)]).is_ok());
}

#[test]
fn conditional_section_delimiters_must_balance() {
    // `ignoreSectContents` requires `<![` and `]]>` to be balanced
    // inside an ignored section, so stopping at the first `]]>` is
    // wrong when the ignored text quotes one -- and everything after it
    // is then misaligned.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d/>"#;
    let dtd = r"<![IGNORE[ the delimiters '<![' and ']]>' must balance ]]>
        <!ELEMENT d EMPTY>";
    assert!(with(doc, &[("d.dtd", dtd)]).is_ok());
}

#[test]
fn an_external_entity_is_still_refused_in_an_attribute() {
    // `WFC: No External Entity References` does not relax because the
    // content happens to be available.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d a="&g;"/>"#;
    assert!(with(doc, &[("g.ent", "hello")]).is_err());
}

#[test]
fn parameter_entities_are_expanded_in_the_external_subset() {
    // `<!ELEMENT d (a,%choice;,c)>` is legal there, and expanding it is
    // what makes the declaration checkable rather than merely skipped.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d/>"#;
    let dtd = r#"
        <!ENTITY % choice "(a|b)">
        <!ELEMENT d (a,%choice;,c)>
    "#;
    assert!(with(doc, &[("d.dtd", dtd)]).is_ok());
}

#[test]
fn a_parameter_entity_can_expand_to_a_whole_declaration() {
    // The specification's own "tricky" example, section 4.5. Character
    // references in an entity value are expanded when it is
    // **declared**, so `&#37;` becomes `%` and `&#60;` becomes `<`:
    // `%xx;` therefore references `zz`, whose text is an entity
    // declaration. Storing the raw text left the whole chain inert.
    let doc = "<?xml version='1.0'?>\n\
        <!DOCTYPE test [\n\
        <!ELEMENT test (#PCDATA) >\n\
        <!ENTITY % xx '&#37;zz;'>\n\
        <!ENTITY % zz '&#60;!ENTITY tricky \"error-prone\" >' >\n\
        %xx;\n\
        ]>\n\
        <test>This sample shows a &tricky; method.</test>";
    let parsed = parse(doc).expect("well-formed");
    assert!(
        parsed.text(parsed.root()).contains("error-prone"),
        "the entity `tricky` should have been declared by expansion"
    );
}

#[test]
fn parameter_entity_recursion_is_bounded() {
    // A parameter entity whose text references itself would otherwise
    // recurse forever, and a DTD is untrusted input like anything else.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d/>"#;
    let dtd = r#"<!ENTITY % a "%b;"><!ENTITY % b "%a;"><!ELEMENT d (%a;)>"#;
    // Either outcome is acceptable; hanging is not.
    let _ = with(doc, &[("d.dtd", dtd)]);
}

#[test]
fn a_conditional_section_must_say_include_or_ignore() {
    // `includeSect ::= '<![' S? 'INCLUDE' S? '[' extSubsetDecl ']]>'`
    // and the same shape for `IGNORE`. Skipping the section without
    // reading it accepted any word at all.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d/>"#;
    for keyword in ["MAYBE", "include", "Ignore", ""] {
        let dtd = format!("<![{keyword}[ <!ELEMENT d EMPTY> ]]>");
        assert!(
            with(doc, &[("d.dtd", &dtd)]).is_err(),
            "{keyword:?} is not a conditional section keyword"
        );
    }
    for keyword in ["INCLUDE", "IGNORE"] {
        let dtd = format!("<![{keyword}[ <!ELEMENT d EMPTY> ]]>");
        assert!(with(doc, &[("d.dtd", &dtd)]).is_ok(), "{keyword}");
    }
}

#[test]
fn an_include_sections_declarations_are_read() {
    // Skipping the section meant the declarations inside it were never
    // seen, so an entity declared there did not exist.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d>&g;</d>"#;
    let dtd = r#"<![INCLUDE[ <!ENTITY g "included"> ]]>"#;
    let parsed = with(doc, &[("d.dtd", dtd)]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "included");
}

#[test]
fn an_ignore_sections_declarations_are_not() {
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d>text</d>"#;
    let dtd = r#"<![IGNORE[ <!ENTITY g "ignored"> ]]><!ELEMENT d (#PCDATA)>"#;
    let parsed = with(doc, &[("d.dtd", dtd)]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "text");
}

#[test]
fn the_keyword_may_come_from_a_parameter_entity() {
    // `<![%MAYBE;[ … ]]>` is how the suite exercises both branches from
    // one document, and it is the reason the keyword cannot simply be
    // read as a literal.
    let doc = r#"<!DOCTYPE d SYSTEM "d.dtd"><d>&g;</d>"#;
    let included = r#"<!ENTITY % yes "INCLUDE"><![%yes;[ <!ENTITY g "on"> ]]>"#;
    let parsed = with(doc, &[("d.dtd", included)]).expect("well-formed");
    assert_eq!(parsed.text(parsed.root()), "on");

    // And a parameter entity holding a word that is not a keyword is
    // still an error.
    let bogus = r#"<!ENTITY % maybe "PERHAPS"><![%maybe;[ <!ENTITY g "x"> ]]>"#;
    assert!(with(doc, &[("d.dtd", bogus)]).is_err());
}

#[test]
fn a_text_declaration_must_come_first_or_not_at_all() {
    // `extParsedEnt ::= TextDecl? content`. Anything before it -- even
    // a blank line -- makes it an ordinary processing instruction with
    // the reserved target `xml`, which is not legal anywhere.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    for bad in [
        "content first\n<?xml version=\"1.0\" encoding=\"UTF-8\"?>",
        "\n<?xml version=\"1.0\" encoding=\"UTF-8\"?>content",
        " <?xml version=\"1.0\" encoding=\"UTF-8\"?>",
    ] {
        assert!(with(doc, &[("g.ent", bad)]).is_err(), "{bad:?}");
    }
    let good = "<?xml version=\"1.0\" encoding=\"UTF-8\"?>content";
    assert!(with(doc, &[("g.ent", good)]).is_ok());
}

#[test]
fn the_text_declaration_keyword_is_lower_case() {
    // The target is reserved case-insensitively, so `<?XML …?>` is an
    // error even in the position a declaration may occupy: `TextDecl`
    // spells it in lower case and nothing else may use the name.
    let doc = r#"<!DOCTYPE d [<!ENTITY g SYSTEM "g.ent">]><d>&g;</d>"#;
    assert!(with(doc, &[("g.ent", r#"<?XML encoding="UTF-8"?>x"#)]).is_err());
}

#[test]
fn an_entity_is_judged_by_its_own_declared_version() {
    // U+0085 is a line terminator in XML 1.1 and an ordinary character
    // in 1.0, so which version an entity declares is observable in what
    // its content becomes. Each entity is normalised by *its own*
    // version, not the document's -- an entity may declare 1.0 inside a
    // 1.1 document and keep 1.0's rules.
    let doc = "<?xml version=\"1.1\"?>\
        <!DOCTYPE d [<!ENTITY g SYSTEM \"g.ent\">]><d>&g;</d>";

    let as_11 = "<?xml version='1.1' encoding='UTF-8'?>a\u{85}b";
    let parsed = with(doc, &[("g.ent", as_11)]).expect("1.1 entity");
    assert_eq!(parsed.text(parsed.root()), "a\nb", "NEL is a line ending");

    let as_10 = "<?xml version='1.0' encoding='UTF-8'?>a\u{85}b";
    let parsed = with(doc, &[("g.ent", as_10)]).expect("1.0 entity");
    assert_eq!(
        parsed.text(parsed.root()),
        "a\u{85}b",
        "in 1.0 it is an ordinary character, even inside a 1.1 document"
    );
}
