// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! External parameter entities, and their text declarations.
//!
//! `<!ENTITY % p SYSTEM "x.ent">` followed by `%p;` pulls declarations
//! into the subset from a separate file. oxml opens nothing, so the
//! content arrives from the caller — but until this was written it was
//! never asked for: `Dtd::parameters` held internal entities only, so
//! `%p;` resolved to nothing and the file's text declaration went
//! unchecked.
//!
//! `TextDecl ::= '<?xml' VersionInfo? EncodingDecl S? '?>'`. The
//! version is optional but must come *before* the encoding, the
//! encoding is required, `standalone` belongs to a document and not to
//! an entity, and the declaration must come first.

use oxml::{Limits, parse, parse_with_external};

fn with_ent(doc: &str, ent: &str) -> Result<(), oxml::ErrorKind> {
    let source: &[(&str, &str)] = &[("p.ent", ent)];
    parse_with_external(doc, Limits::default(), &source)
        .map(|_| ())
        .map_err(|e| e.kind)
}

/// A document that declares the entity and references it.
const USES: &str = r#"<!DOCTYPE root [
<!ELEMENT root (#PCDATA)>
<!ENTITY % p SYSTEM "p.ent">
%p;
]>
<root/>"#;

/// The same document that declares it and never references it.
const DECLARES_ONLY: &str = r#"<!DOCTYPE root [
<!ELEMENT root (#PCDATA)>
<!ENTITY % p SYSTEM "p.ent">
]>
<root/>"#;

#[test]
fn a_well_formed_text_declaration_is_accepted() {
    for ent in [
        "<?xml encoding='UTF-8'?>\n<!ELEMENT a EMPTY>",
        "<?xml version='1.0' encoding='UTF-8'?>\n<!ELEMENT a EMPTY>",
        // A text declaration is optional.
        "<!ELEMENT a EMPTY>",
    ] {
        assert!(with_ent(USES, ent).is_ok(), "{ent:?} should be accepted");
    }
}

#[test]
fn a_malformed_text_declaration_is_refused() {
    for (ent, why) in [
        (
            "<?xml encoding=\"UTF-8\">\n<!ELEMENT a EMPTY>",
            "the closing `?` is missing",
        ),
        (
            "<?xml encoding='UTF8' version='1.0' ?>\n<!ELEMENT a EMPTY>",
            "the version must precede the encoding",
        ),
        (
            "<!-- a comment -->\n<?xml encoding='UTF-8'?>",
            "a text declaration must come first",
        ),
        (
            "<?xml version='1.0' encoding='utf-8' standalone='yes'?>",
            "only a document may be standalone",
        ),
    ] {
        assert!(with_ent(USES, ent).is_err(), "{why}: {ent:?}");
    }
}

/// An entity nothing references need not be read.
///
/// The specification says a processor need not read one, so a
/// malformed declaration in an entity the document never uses is not
/// an error. Checking eagerly would reject valid documents -- which is
/// why the content is fetched when declared and judged when
/// referenced.
#[test]
fn an_unreferenced_entity_is_not_judged() {
    let malformed = "<?xml encoding=\"UTF-8\">\n<!ELEMENT a EMPTY>";
    assert!(
        with_ent(DECLARES_ONLY, malformed).is_ok(),
        "an entity nothing references need not be read"
    );
    // The same content, referenced, is refused.
    assert!(with_ent(USES, malformed).is_err());
}

/// Declarations from the entity actually reach the subset.
#[test]
fn the_declarations_are_pulled_in() {
    // The entity declares a general entity the document then uses. If
    // the content were not spliced, `&greeting;` would be unknown.
    let ent = "<?xml encoding='UTF-8'?>\n<!ENTITY greeting \"hello\">";
    let doc = r#"<!DOCTYPE root [
<!ELEMENT root (#PCDATA)>
<!ENTITY % p SYSTEM "p.ent">
%p;
]>
<root>&greeting;</root>"#;
    let source: &[(&str, &str)] = &[("p.ent", ent)];
    let parsed = parse_with_external(doc, Limits::default(), &source)
        .expect("the entity supplies the declaration");
    assert_eq!(
        parsed.text(parsed.root()),
        "hello",
        "the declaration pulled in by `%p;` was not used"
    );
}

/// With nothing supplied, the subset is incomplete rather than wrong.
#[test]
fn an_unavailable_entity_is_not_an_error() {
    assert!(
        parse(USES).is_ok(),
        "oxml opens nothing; an entity the caller did not supply \
         leaves the subset incomplete, not invalid"
    );
}

/// An external entity's characters must suit **both** versions.
///
/// XML 1.1 §4.3.4. Each entity carries its own version, and the
/// characters it may contain are ruled by that version *and* by the
/// version of the document including it. `#x7F` is legal in XML 1.0
/// and must be escaped in XML 1.1, so a 1.0 external DTD holding one
/// cannot be pulled into a 1.1 document.
#[test]
fn external_content_is_judged_by_both_versions() {
    // DEL: legal in 1.0, must be escaped in 1.1.
    let dtd_10 = "<?xml version='1.0' encoding='UTF-8'?>\n<!ELEMENT root (#PCDATA)>\n<!ENTITY c \"a\u{7f}b\">";

    let doc_10 =
        "<?xml version='1.0'?>\n<!DOCTYPE root SYSTEM \"p.ent\">\n<root/>";
    assert!(
        with_ent(doc_10, dtd_10).is_ok(),
        "a 1.0 entity in a 1.0 document may hold #x7F"
    );

    let doc_11 =
        "<?xml version='1.1'?>\n<!DOCTYPE root SYSTEM \"p.ent\">\n<root/>";
    assert!(
        with_ent(doc_11, dtd_10).is_err(),
        "#x7F must not reach a 1.1 document, whatever the entity says"
    );
}

/// An external entity's replacement text is markup, as an internal
/// one's is.
///
/// The rule is the same for both -- XML 1.0 §4.4.2, *Included* -- and
/// checking only internal entities left this accepted: `NEL` (#x85) is
/// whitespace in XML 1.1 and not in XML 1.0, so a 1.0 document that
/// pulls in `<root\u{85}/>` has a character where a tag needs
/// whitespace.
#[test]
fn an_external_entity_is_content_not_characters() {
    let doc = r#"<!DOCTYPE doc [
<!ELEMENT doc (root*)>
<!ELEMENT root EMPTY>
<!ENTITY e SYSTEM "p.ent">
]>
<doc>&e;</doc>"#;

    // NEL inside a tag: not whitespace in 1.0, so not a tag.
    let bad = "<?xml encoding='UTF-8'?>\n<root/><root\u{85}/>";
    assert!(
        with_ent(doc, bad).is_err(),
        "NEL is not whitespace in XML 1.0"
    );

    // The same shape, well-formed, stays accepted.
    let good = "<?xml encoding='UTF-8'?>\n<root/><root/>";
    assert!(with_ent(doc, good).is_ok(), "well-formed content is fine");

    // And an external entity that is not well-formed content at all.
    let unbalanced = "<?xml encoding='UTF-8'?>\n</root>";
    assert!(
        with_ent(doc, unbalanced).is_err(),
        "an entity may not close an element it did not open"
    );
}
