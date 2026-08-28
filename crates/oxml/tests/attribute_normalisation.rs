// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Attribute-value normalisation, and the part of it that depends on
//! the declaration.
//!
//! XML 1.0 section 3.3.3 has two passes. Every value gets the first:
//! tabs and line breaks become spaces. Only a value whose declared
//! type is **not** `CDATA` gets the second: leading and trailing
//! spaces discarded, runs of spaces collapsed to one.
//!
//! The second pass cannot be decided by looking at the value. Nothing
//! about `" urn:x "` says whether the spaces matter — only the
//! `ATTLIST` does. That is why this needs the DTD, and why a parser
//! that skips it is silently wrong rather than loudly incomplete.

use oxml::parse;

/// A `CDATA` value keeps its spaces; a tokenized one does not.
#[test]
fn only_a_non_cdata_type_is_collapsed() {
    let doc = r#"<!DOCTYPE r [
<!ELEMENT r EMPTY>
<!ATTLIST r keep CDATA #IMPLIED
            trim NMTOKEN #IMPLIED>
]>
<r keep="  a  b  " trim="  a  b  "/>"#;
    let d = parse(doc).expect("well-formed");
    let root = d.root_element().expect("a root");
    assert_eq!(
        d.attribute(root, "keep"),
        Some("  a  b  "),
        "CDATA keeps every space"
    );
    assert_eq!(
        d.attribute(root, "trim"),
        Some("a b"),
        "a tokenized value is stripped and collapsed"
    );
}

/// Every non-`CDATA` type, including the two written with parentheses.
///
/// An enumeration and `NOTATION` are neither `ID` nor `CDATA`, and
/// they are the easy ones to miss because they have no keyword of
/// their own.
#[test]
fn every_tokenized_type_is_collapsed() {
    for ty in [
        "ID", "IDREF", "IDREFS", "ENTITY", "ENTITIES", "NMTOKEN", "NMTOKENS",
        "(a|b)",
    ] {
        let doc = format!(
            "<!DOCTYPE r [<!ELEMENT r EMPTY><!ATTLIST r v {ty} #IMPLIED>]>\
             <r v=\"  a  \"/>"
        );
        let d = parse(&doc).unwrap_or_else(|e| panic!("{ty}: {e}"));
        let root = d.root_element().expect("a root");
        assert_eq!(d.attribute(root, "v"), Some("a"), "type {ty}");
    }
}

/// Tabs and newlines become spaces first, then collapse.
///
/// The two passes compose: without the first there would be nothing
/// for the second to collapse.
#[test]
fn the_two_passes_compose() {
    let doc = "<!DOCTYPE r [<!ELEMENT r EMPTY>\
               <!ATTLIST r v NMTOKENS #IMPLIED>]>\
               <r v=\"\ta\n\nb\t\"/>";
    let d = parse(doc).expect("well-formed");
    let root = d.root_element().expect("a root");
    assert_eq!(d.attribute(root, "v"), Some("a b"));
}

/// Without a declaration, nothing is collapsed.
///
/// A processor cannot invent the type: an undeclared attribute is
/// `CDATA` by default, so its spaces are content.
#[test]
fn an_undeclared_attribute_keeps_its_spaces() {
    let d = parse("<r v=\"  a  b  \"/>").expect("well-formed");
    let root = d.root_element().expect("a root");
    assert_eq!(d.attribute(root, "v"), Some("  a  b  "));
}

/// The W3C case: normalisation makes two namespace declarations equal.
///
/// `eduni/rmt-ns10-012`. `xmlns:b` is declared `NMTOKEN`, so
/// `" urn:xyzzy "` becomes `urn:xyzzy` — the same URI `xmlns:a`
/// declares. `a:attr` and `b:attr` then have one expanded name
/// between them, and an element may not carry the same attribute
/// twice.
#[test]
fn normalisation_can_make_two_prefixes_one_namespace() {
    let doc = r#"<!DOCTYPE foo [
<!ELEMENT foo ANY>
<!ATTLIST foo xmlns:a CDATA #IMPLIED
              xmlns:b NMTOKEN #IMPLIED>
<!ELEMENT bar ANY>
<!ATTLIST bar a:attr CDATA #IMPLIED
              b:attr CDATA #IMPLIED>
]>
<foo xmlns:a="urn:xyzzy" xmlns:b=" urn:xyzzy ">
<bar a:attr="1" b:attr="2"/>
</foo>"#;
    assert!(
        parse(doc).is_err(),
        "the two prefixes name one namespace, so `bar` declares the \
         same attribute twice"
    );

    // The same document with `xmlns:b` declared CDATA keeps its
    // spaces, so the namespaces differ and there is no collision.
    let cdata = doc.replace("xmlns:b NMTOKEN", "xmlns:b CDATA");
    assert!(
        parse(&cdata).is_ok(),
        "as CDATA the spaces are kept and the URIs are different"
    );
}
