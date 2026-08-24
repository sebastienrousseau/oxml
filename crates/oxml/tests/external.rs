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
