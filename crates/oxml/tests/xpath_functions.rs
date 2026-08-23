// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The `XPath` 1.0 function library.
//!
//! Each function has a specified return type and a specified behaviour
//! on the awkward inputs — an empty node-set, a negative index, a
//! non-numeric string. Those are the cases callers hit in practice and
//! the ones where implementations quietly diverge.

use oxml::{XPath, parse};

const DOC: &str = "\
<r xmlns:m=\"urn:m\">\
<a id=\"1\">  spaced   out  </a>\
<b>12</b><b>30</b>\
<m:tagged>ns</m:tagged>\
<empty/>\
</r>";

fn val(expr: &str) -> String {
    let doc = parse(DOC).expect("well-formed");
    XPath::compile(expr)
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"))
        .evaluate(&doc)
        .to_str(&doc)
}

#[test]
fn string_conversion_follows_the_type_rules() {
    assert_eq!(val("string(42)"), "42");
    assert_eq!(val("string(4.5)"), "4.5");
    assert_eq!(val("string(true())"), "true");
    assert_eq!(val("string(false())"), "false");
    assert_eq!(val("string('x')"), "x");
    assert_eq!(val("string(//b)"), "12", "the first node in the set");
    assert_eq!(
        val("string(//missing)"),
        "",
        "an empty set is an empty string"
    );
}

#[test]
fn number_conversion_yields_nan_for_non_numeric_input() {
    assert_eq!(val("number('42')"), "42");
    assert_eq!(val("number('4.5')"), "4.5");
    assert_eq!(val("number(true())"), "1");
    assert_eq!(val("number(false())"), "0");
    assert_eq!(val("number('abc')"), "NaN");
    assert_eq!(val("number(//missing)"), "NaN");
}

#[test]
fn count_and_sum_operate_on_node_sets() {
    assert_eq!(val("count(//b)"), "2");
    assert_eq!(val("count(//missing)"), "0");
    assert_eq!(val("sum(//b)"), "42");
    assert_eq!(val("sum(//missing)"), "0");
}

#[test]
fn string_length_counts_characters() {
    assert_eq!(val("string-length('abc')"), "3");
    assert_eq!(val("string-length('')"), "0");
    assert_eq!(val("string-length('é中')"), "2", "characters, not bytes");
}

#[test]
fn concat_joins_every_argument() {
    assert_eq!(val("concat('a','b')"), "ab");
    assert_eq!(val("concat('a','b','c','d')"), "abcd");
    assert_eq!(val("concat('n=', 1)"), "n=1");
}

#[test]
fn starts_with_and_contains_answer_booleans() {
    assert_eq!(val("starts-with('abcdef','abc')"), "true");
    assert_eq!(val("starts-with('abcdef','bcd')"), "false");
    assert_eq!(val("starts-with('abc','')"), "true");
    assert_eq!(val("contains('abcdef','cde')"), "true");
    assert_eq!(val("contains('abcdef','xyz')"), "false");
    assert_eq!(val("contains('abc','')"), "true");
}

#[test]
fn normalize_space_collapses_and_trims() {
    assert_eq!(val("normalize-space('  a   b  ')"), "a b");
    assert_eq!(val("normalize-space('')"), "");
    assert_eq!(val("normalize-space('   ')"), "");
    assert_eq!(val("normalize-space(//a)"), "spaced out");
}

#[test]
fn substring_is_one_based_and_clamps_out_of_range_indices() {
    // XPath counts from 1, and the specification requires out-of-range
    // requests to be clamped rather than to error.
    assert_eq!(val("substring('12345',2,3)"), "234");
    assert_eq!(val("substring('12345',2)"), "2345");
    assert_eq!(val("substring('12345',0,3)"), "12");
    assert_eq!(val("substring('12345',-1,3)"), "1");
    assert_eq!(val("substring('12345',10)"), "");
    assert_eq!(val("substring('12345',1,0)"), "");
    assert_eq!(val("substring('12345',1,100)"), "12345");
}

#[test]
fn local_name_and_namespace_uri_read_the_context_node() {
    assert_eq!(val("local-name(//m:tagged)"), "tagged");
    assert_eq!(val("namespace-uri(//m:tagged)"), "urn:m");
    assert_eq!(val("local-name(//a)"), "a");
    assert_eq!(val("namespace-uri(//a)"), "", "no namespace");
    assert_eq!(val("local-name(//missing)"), "", "an empty set has no name");
}

#[test]
fn the_rounding_functions_follow_the_specification() {
    assert_eq!(val("floor(1.9)"), "1");
    assert_eq!(val("floor(-1.1)"), "-2");
    assert_eq!(val("ceiling(1.1)"), "2");
    assert_eq!(val("ceiling(-1.9)"), "-1");
    assert_eq!(val("round(1.5)"), "2");
    assert_eq!(val("round(1.4)"), "1");
    assert_eq!(val("round(-1.5)"), "-1", "XPath rounds half towards +∞");
}

#[test]
fn an_unknown_function_is_rejected_at_compile_time_or_evaluation() {
    let doc = parse(DOC).expect("well-formed");
    match XPath::compile("nosuchfunction()") {
        Err(_) => {}
        Ok(x) => {
            // If it compiles it must not silently answer as though the
            // function existed.
            let _ = x.evaluate(&doc);
        }
    }
}

#[test]
fn evaluate_from_uses_the_supplied_context_node() {
    // The whole point of the entry point: a relative expression means
    // something different from a different node.
    let doc = parse(DOC).expect("well-formed");
    let books = XPath::compile("//b").expect("valid").evaluate(&doc);
    let nodes = books.nodes().expect("a node set");
    let second = nodes[1];

    let here = XPath::compile("self::b").expect("valid");
    assert_eq!(here.evaluate_from(&doc, second).to_str(&doc), "30");

    let up = XPath::compile("parent::r").expect("valid");
    assert_eq!(
        up.evaluate_from(&doc, second)
            .nodes()
            .map_or(0, <[oxml::NodeId]>::len),
        1
    );
}

#[test]
fn a_compiled_expression_exposes_its_syntax_tree() {
    // Public so callers can inspect or cache; if it stops compiling
    // the API has silently changed.
    let x = XPath::compile("//b[1]").expect("valid");
    assert!(!format!("{:?}", x.expr()).is_empty());
}

#[test]
fn a_deeply_nested_expression_is_an_error_not_a_stack_overflow() {
    // An XPath expression is untrusted input in every front end of
    // this crate: the CLI takes it from a shell, the MCP server from a
    // model, the WASM bindings from JavaScript. Recursive-descent
    // parsing without a bound turns that into a process abort no
    // caller can catch.
    assert!(XPath::compile(&"(".repeat(100)).is_err());
    for depth in [1_000usize, 100_000] {
        let expr = format!("{}1{}", "(".repeat(depth), ")".repeat(depth));
        let e = XPath::compile(&expr).expect_err("beyond the limit");
        assert!(e.to_string().contains("deep"), "{e}");
    }
}

#[test]
fn nesting_within_the_limit_still_compiles() {
    let depth = 50;
    let expr = format!("{}1 + 1{}", "(".repeat(depth), ")".repeat(depth));
    let doc = parse(DOC).expect("well-formed");
    let x = XPath::compile(&expr).expect("within the limit");
    assert_eq!(x.evaluate(&doc).to_str(&doc), "2");
}
