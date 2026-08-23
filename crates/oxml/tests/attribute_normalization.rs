// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Attribute-value normalisation, XML section 3.3.3.
//!
//! Each whitespace character in an attribute value is replaced by a
//! space. Without it an attribute written across two lines came back
//! carrying the newline and the next line's indentation, so two
//! documents differing only in line wrapping produced different values.

use oxml::parse;

/// The value of attribute `b` on the root element.
fn value(source: &str) -> String {
    let doc = parse(source).expect("well-formed");
    let root = doc.root_element().expect("a root element");
    doc.attribute(root, "b").expect("attribute b").to_owned()
}

#[test]
fn literal_whitespace_becomes_a_space() {
    assert_eq!(value("<a b='x\ny'/>"), "x y", "newline");
    assert_eq!(value("<a b='x\ty'/>"), "x y", "tab");
    // Line endings are normalised first, so CR LF is one character by
    // the time this rule applies -- not two spaces.
    assert_eq!(value("<a b='x\r\ny'/>"), "x y", "CR LF is one space");
    assert_eq!(value("<a b='x\ry'/>"), "x y", "a lone CR");
}

#[test]
fn a_value_wrapped_across_lines_keeps_its_indentation_as_spaces() {
    // The rule replaces rather than collapses, for a CDATA attribute.
    // Five spaces of indentation stay five spaces; they do not become
    // one, and they do not stay a newline.
    assert_eq!(value("<a b='one\n     two'/>"), "one      two");
}

#[test]
fn spaces_are_left_alone() {
    assert_eq!(value("<a b='hello world'/>"), "hello world");
    assert_eq!(value("<a b='  padded  '/>"), "  padded  ");
    assert_eq!(value("<a b=''/>"), "");
}

#[test]
fn a_character_reference_is_exempt() {
    // `&#xA;` is the only way to put a newline in an attribute value.
    // Normalising it would make the character unrepresentable, so the
    // specification exempts character references.
    assert_eq!(value("<a b='x&#xA;y'/>"), "x\ny", "hex");
    assert_eq!(value("<a b='x&#10;y'/>"), "x\ny", "decimal");
    assert_eq!(value("<a b='x&#9;y'/>"), "x\ty", "tab");
    assert_eq!(value("<a b='x&#xD;y'/>"), "x\ry", "carriage return");
}

#[test]
fn a_general_entitys_replacement_text_is_normalised() {
    // The specification processes an entity's replacement text
    // recursively, so whitespace inside it is replaced -- unlike a
    // character reference, which is not.
    let source = "<!DOCTYPE a [<!ENTITY e \"p\nq\">]><a b='&e;'/>";
    assert_eq!(value(source), "p q");

    // But a character reference *inside* the entity is still exempt.
    let escaped = "<!DOCTYPE a [<!ENTITY e \"p&#xA;q\">]><a b='&e;'/>";
    assert_eq!(value(escaped), "p\nq");
}

#[test]
fn the_predefined_entities_are_unaffected() {
    assert_eq!(value("<a b='x&amp;y'/>"), "x&y");
    assert_eq!(value("<a b='x&lt;y'/>"), "x<y");
    assert_eq!(value("<a b='x&quot;y'/>"), "x\"y");
}

#[test]
fn text_content_is_not_normalised() {
    // The rule is for attribute values only. A newline in element
    // content is content.
    let doc = parse("<a>x\ny</a>").expect("well-formed");
    assert_eq!(doc.text(doc.root()), "x\ny");
    let doc = parse("<a>x\ty</a>").expect("well-formed");
    assert_eq!(doc.text(doc.root()), "x\ty");
}
