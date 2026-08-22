// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `XPath` behaviour, pinned against the 1.0 specification.

#![cfg(feature = "xpath")]

use oxml::{Document, XPath, parse};

const LIBRARY: &str = r#"
<library xmlns:m="urn:meta">
  <book lang="en" year="1965" m:id="b1">
    <title>Dune</title>
    <price>9.99</price>
  </book>
  <book lang="fr" year="1885" m:id="b2">
    <title>Germinal</title>
    <price>7.50</price>
  </book>
  <book lang="en" year="1949" m:id="b3">
    <title>Nineteen Eighty-Four</title>
    <price>8.25</price>
  </book>
  <!-- an aside -->
</library>
"#;

fn doc() -> Document {
    parse(LIBRARY).expect("fixture parses")
}

fn strings(xp: &str, d: &Document) -> Vec<String> {
    XPath::compile(xp)
        .expect("compiles")
        .evaluate(d)
        .nodes()
        .expect("a node-set")
        .iter()
        .map(|n| d.text(*n))
        .collect()
}

#[test]
fn descendant_search_finds_every_match() {
    let d = doc();
    assert_eq!(
        strings("//title", &d),
        ["Dune", "Germinal", "Nineteen Eighty-Four"]
    );
}

#[test]
fn attribute_predicate_filters() {
    let d = doc();
    assert_eq!(
        strings("//book[@lang='en']/title", &d),
        ["Dune", "Nineteen Eighty-Four"]
    );
}

/// The attribute axis must yield the *attribute*, not its element.
///
/// An earlier implementation returned the owning element from
/// `attribute::`, which made `string(//book/@lang)` evaluate to the
/// book's text content instead of `"en"` — wrong, and silently so.
#[test]
fn attribute_axis_yields_the_attribute_value() {
    let d = doc();
    let v = XPath::compile("//book/@lang")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(v.to_str(&d), "en", "string() of an attribute node");

    let all: Vec<String> = v
        .nodes()
        .expect("a node-set")
        .iter()
        .map(|n| d.text(*n))
        .collect();
    assert_eq!(all, ["en", "fr", "en"]);
}

#[test]
fn numeric_comparison_uses_number_conversion() {
    let d = doc();
    assert_eq!(
        strings("//book[@year>1900]/title", &d),
        ["Dune", "Nineteen Eighty-Four"]
    );
}

#[test]
fn positional_predicate_is_one_based() {
    let d = doc();
    assert_eq!(strings("//book[1]/title", &d), ["Dune"]);
    assert_eq!(strings("//book[2]/title", &d), ["Germinal"]);
}

#[test]
fn equality_against_a_node_set_is_existential() {
    let d = doc();
    // True because *some* book is in English, not because all are.
    let v = XPath::compile("//book/@lang = 'fr'")
        .expect("compiles")
        .evaluate(&d);
    assert!(v.to_boolean());
}

#[test]
fn count_and_sum_aggregate() {
    let d = doc();
    let count = XPath::compile("count(//book)")
        .expect("compiles")
        .evaluate(&d);
    assert!((count.to_number(&d) - 3.0).abs() < f64::EPSILON);

    let sum = XPath::compile("sum(//price)")
        .expect("compiles")
        .evaluate(&d);
    assert!((sum.to_number(&d) - 25.74).abs() < 1e-9);
}

#[test]
fn string_functions_follow_the_spec() {
    let d = doc();
    let c = XPath::compile("concat('a', 'b', 'c')")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(c.to_str(&d), "abc");

    let s = XPath::compile("substring('hello', 2, 3)")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(s.to_str(&d), "ell");

    let n = XPath::compile("normalize-space('  a   b  ')")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(n.to_str(&d), "a b");
}

#[test]
fn number_formatting_drops_a_trailing_zero() {
    let d = doc();
    // XPath prints 3 as "3", never "3.0".
    let v = XPath::compile("count(//book)")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(v.to_str(&d), "3");
}

#[test]
fn parent_and_self_axes_navigate() {
    let d = doc();
    assert_eq!(strings("//title/..", &d).len(), 3);
    assert_eq!(strings("//title/self::title", &d).len(), 3);
}

#[test]
fn union_merges_and_deduplicates() {
    let d = doc();
    let v = XPath::compile("//title | //price")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(v.nodes().expect("node-set").len(), 6);

    // The same set twice must not double.
    let same = XPath::compile("//title | //title")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(same.nodes().expect("node-set").len(), 3);
}

#[test]
fn boolean_operators_short_circuit_and_combine() {
    let d = doc();
    let both = XPath::compile("count(//book) = 3 and count(//title) = 3")
        .expect("compiles")
        .evaluate(&d);
    assert!(both.to_boolean());

    let either = XPath::compile("count(//book) = 99 or count(//title) = 3")
        .expect("compiles")
        .evaluate(&d);
    assert!(either.to_boolean());
}

#[test]
fn comment_nodes_are_selectable() {
    let d = doc();
    let v = XPath::compile("//comment()")
        .expect("compiles")
        .evaluate(&d);
    assert_eq!(v.nodes().expect("node-set").len(), 1);
}

#[test]
fn word_operators_are_not_confused_with_names() {
    // `andover` must lex as a name, not `and` followed by `over`.
    let d = parse("<r><andover>x</andover></r>").expect("parses");
    assert_eq!(strings("//andover", &d), ["x"]);

    // ...and `div` as an element name, not a division.
    let d2 = parse("<r><div>y</div></r>").expect("parses");
    assert_eq!(strings("//div", &d2), ["y"]);
}

#[test]
fn malformed_expressions_report_rather_than_panic() {
    assert!(XPath::compile("//book[").is_err());
    assert!(XPath::compile("'unterminated").is_err());
    assert!(XPath::compile("//book)extra").is_err());
}
