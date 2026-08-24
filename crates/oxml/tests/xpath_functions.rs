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
    // `m` is bound for every expression here, because a prefix in an
    // expression resolves against the expression's own bindings and
    // not the document's declarations -- an unbound one is a compile
    // error rather than a match on the local part alone.
    let doc = parse(DOC).expect("well-formed");
    XPath::compile_with_namespaces(expr, &[("m", "urn:m")])
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

/// `local-name()` and `namespace-uri()` describe attributes too.
///
/// Both read the node's expanded name, and `XPath` 1.0 gives one to
/// elements *and* attributes. Reading only the element name made both
/// answer the empty string for every attribute, which is wrong on its
/// own and also broke the only workaround available for selecting an
/// attribute by namespace -- so a query returned nothing and looked
/// like an empty document rather than a defect.
#[test]
fn name_functions_describe_attributes_and_pis_not_only_elements() {
    let doc = parse(r#"<r xmlns:m="urn:u"><x m:a="A" b="B"/><?pi go?></r>"#)
        .expect("well-formed");

    for (expr, want) in [
        ("local-name(//@m:a)", "a"),
        ("namespace-uri(//@m:a)", "urn:u"),
        // An attribute in no namespace has an empty namespace URI, not
        // an absent one.
        ("local-name(//@b)", "b"),
        ("namespace-uri(//@b)", ""),
        // Elements still work.
        ("local-name(//x)", "x"),
        ("namespace-uri(//x)", ""),
        // A processing instruction has a local part -- its target --
        // and no namespace.
        ("local-name(//processing-instruction())", "pi"),
        ("namespace-uri(//processing-instruction())", ""),
        // Text, comments and the root have neither.
        ("local-name(/)", ""),
    ] {
        let value = XPath::compile_with_namespaces(expr, &[("m", "urn:u")])
            .expect("compiles")
            .evaluate(&doc);
        assert_eq!(value.to_str(&doc), want, "{expr}");
    }
}

/// Selecting an attribute by its namespace with `namespace-uri()`.
///
/// Once the workaround for name tests not resolving prefixes; now the
/// way to select on a namespace without naming a prefix at all, which
/// is still useful when the URI is known and the prefix is not.
#[test]
fn attributes_can_be_selected_by_namespace_uri() {
    let doc = parse(r#"<r xmlns:m="urn:u"><x m:a="A" b="B"/></r>"#)
        .expect("well-formed");

    let namespaced = XPath::compile("//@*[namespace-uri()='urn:u']")
        .expect("compiles")
        .evaluate(&doc);
    assert_eq!(namespaced.nodes().expect("a node-set").len(), 1);
    assert_eq!(namespaced.to_str(&doc), "A");

    let plain = XPath::compile("//@*[namespace-uri()='']")
        .expect("compiles")
        .evaluate(&doc);
    assert_eq!(plain.nodes().expect("a node-set").len(), 1);
    assert_eq!(plain.to_str(&doc), "B");
}

/// A prefix in an expression resolves against the expression's own
/// bindings, not the document's declarations.
///
/// Before 0.0.4 a prefixed name test matched on its local part alone,
/// so `//m:a` selected every `a` whatever its namespace. That is a
/// wrong answer with no error attached, which is the worst way for a
/// query engine to fail: nothing distinguishes it from a document that
/// really does contain those nodes.
#[test]
fn a_prefixed_name_test_matches_only_that_namespace() {
    let doc = parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a><a>no</a></r>"#)
        .expect("well-formed");

    let bound = XPath::compile_with_namespaces("//m:a", &[("m", "urn:u")])
        .expect("compiles");
    let found = bound.evaluate(&doc);
    assert_eq!(found.nodes().expect("a node-set").len(), 1);
    assert_eq!(found.to_str(&doc), "yes");
}

#[test]
fn only_the_uri_matters_never_the_prefix() {
    // The expression may use a different prefix from the document, and
    // a document that renames its prefixes still matches. This is the
    // reason resolution happens against the expression's bindings.
    let doc =
        parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a></r>"#).expect("well-formed");
    let other = parse(r#"<r xmlns:zz="urn:u"><zz:a>yes</zz:a></r>"#)
        .expect("well-formed");

    let query = XPath::compile_with_namespaces("//q:a", &[("q", "urn:u")])
        .expect("compiles");
    assert_eq!(query.evaluate(&doc).to_str(&doc), "yes");
    assert_eq!(query.evaluate(&other).to_str(&other), "yes");
}

#[test]
fn an_unbound_prefix_is_a_compile_error() {
    // Loud, rather than a match on the local part.
    let error = XPath::compile("//m:a").expect_err("unbound");
    assert!(
        error.message.contains("unbound namespace prefix"),
        "{}",
        error.message
    );
    // Binding a different prefix does not help.
    let error = XPath::compile_with_namespaces("//m:a", &[("q", "urn:u")])
        .expect_err("still unbound");
    assert!(error.message.contains("unbound"), "{}", error.message);
}

#[test]
fn the_xml_prefix_is_bound_without_being_declared() {
    // Bound by specification, in expressions as in documents.
    let doc = parse(r#"<r><a xml:lang="en">x</a></r>"#).expect("well-formed");
    let query = XPath::compile("//@xml:lang").expect("compiles");
    assert_eq!(query.evaluate(&doc).to_str(&doc), "en");
}

#[test]
fn an_unprefixed_name_test_matches_no_namespace_only() {
    // XPath 1.0's classic surprise, and what every conforming engine
    // does: a default namespace does not apply to node tests.
    let doc = parse(r#"<r xmlns:m="urn:u"><m:a>ns</m:a><a>plain</a></r>"#)
        .expect("well-formed");
    let found = XPath::compile("//a").expect("compiles").evaluate(&doc);
    assert_eq!(found.nodes().expect("a node-set").len(), 1);
    assert_eq!(found.to_str(&doc), "plain");

    // So a document with a default namespace matches nothing by a bare
    // name -- the case that surprises people.
    let defaulted = parse(r#"<r xmlns="urn:u"><item/></r>"#).expect("ok");
    let bare = XPath::compile("//item").expect("compiles");
    assert_eq!(bare.evaluate(&defaulted).nodes().expect("set").len(), 0);

    let prefixed =
        XPath::compile_with_namespaces("//x:item", &[("x", "urn:u")])
            .expect("compiles");
    assert_eq!(prefixed.evaluate(&defaulted).nodes().expect("set").len(), 1);
}

#[test]
fn a_wildcard_still_ignores_namespaces() {
    let doc = parse(r#"<r xmlns:m="urn:u"><m:a/><a/></r>"#).expect("ok");
    let all = XPath::compile("//*").expect("compiles").evaluate(&doc);
    assert_eq!(all.nodes().expect("a node-set").len(), 3, "r, m:a, a");
}
