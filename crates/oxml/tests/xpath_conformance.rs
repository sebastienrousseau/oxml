// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `XPath` 1.0 correctness, pinned case by case against the
//! specification's wording.
//!
//! Each test here reproduces a specific way an engine can be *nearly*
//! right: predicates filtered over the merged result of a step rather
//! than per context node, reverse axes counted from the far end,
//! relational operators that read only the first node of a set. Every
//! one of those gives a plausible answer on simple documents, which is
//! why each case is on a document shaped to make the difference
//! visible.

#![cfg(all(feature = "xpath", feature = "std"))]

use oxml::xpath::{EvalContext, EvalError, Value};
use oxml::{Document, ExpandedName, XPath, parse};

/// Compile `expr`, checking on the way that `Display` round-trips it.
fn compile(expr: &str) -> XPath {
    let compiled = XPath::compile(expr)
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"));
    let rendered = compiled.to_string();
    let again = XPath::compile(&rendered)
        .unwrap_or_else(|e| panic!("`{expr}` rendered as `{rendered}`: {e}"));
    assert_eq!(again, compiled, "`{expr}` rendered as `{rendered}`");
    compiled
}

/// The text of every node `expr` selects from `src`.
fn texts(src: &str, expr: &str) -> Vec<String> {
    let doc = parse(src).expect("well-formed");
    let value = compile(expr).evaluate(&doc);
    value
        .nodes()
        .unwrap_or_else(|| panic!("`{expr}` is not a node-set: {value:?}"))
        .iter()
        .map(|id| doc.text(*id))
        .collect()
}

/// The local name of every node `expr` selects from `src`.
fn names(src: &str, expr: &str) -> Vec<String> {
    let doc = parse(src).expect("well-formed");
    let value = compile(expr).evaluate(&doc);
    value
        .nodes()
        .unwrap_or_else(|| panic!("`{expr}` is not a node-set: {value:?}"))
        .iter()
        .map(|id| {
            doc.element_name(*id)
                .map_or_else(String::new, |n| n.local.clone())
        })
        .collect()
}

/// The string-value of `expr` evaluated against `src`.
fn val(src: &str, expr: &str) -> String {
    let doc = parse(src).expect("well-formed");
    compile(expr).evaluate(&doc).to_str(&doc)
}

const NESTED: &str = "<r><p><a>1</a><a>2</a></p><p><a>3</a><a>4</a></p></r>";

/// `XPath` 1.0 section 2.4: a predicate filters the node-set the axis
/// produces *from each context node*, with the proximity position
/// counted within that node-set. `//a[1]` is therefore every `a` that
/// is the first `a` child of its parent, not the first `a` in the
/// document.
#[test]
fn predicates_apply_per_context_node() {
    assert_eq!(texts(NESTED, "//a[1]"), ["1", "3"]);
    assert_eq!(texts(NESTED, "//a[last()]"), ["2", "4"]);
    assert_eq!(texts(NESTED, "/r/p/a[2]"), ["2", "4"]);
    assert_eq!(texts(NESTED, "//a[position() = 2]"), ["2", "4"]);
    // The abbreviation is not the same as the descendant axis: a single
    // step from one context node has one node-set to count in.
    assert_eq!(texts(NESTED, "/descendant::a[1]"), ["1"]);
    assert_eq!(texts(NESTED, "/descendant::a[last()]"), ["4"]);
}

/// The constant-index fast path must agree with the general case on
/// every shape of number: `1.0` is position one, and nothing is at
/// position zero, one and a half, minus one, or ten to the twenty-one.
#[test]
fn the_constant_index_fast_path_covers_every_number_shape() {
    assert_eq!(texts(NESTED, "//a[1.0]"), ["1", "3"]);
    assert_eq!(texts(NESTED, "//a[0]"), Vec::<String>::new());
    assert_eq!(texts(NESTED, "//a[1.5]"), Vec::<String>::new());
    assert_eq!(texts(NESTED, "//a[-1]"), Vec::<String>::new());
    assert_eq!(
        texts(NESTED, "//a[1000000000000000000000]"),
        Vec::<String>::new()
    );
    assert_eq!(texts(NESTED, "//a[2]"), ["2", "4"]);
    assert_eq!(texts(NESTED, "//a[3]"), Vec::<String>::new());
}

/// Section 2.4: on a reverse axis the proximity position counts in
/// reverse document order, so `[1]` is the *nearest* node.
#[test]
fn reverse_axes_count_position_from_the_nearest_node() {
    let src = "<r><b><c><x/></c></b></r>";
    assert_eq!(names(src, "//x/ancestor::*[1]"), ["c"]);
    assert_eq!(names(src, "//x/ancestor::*[last()]"), ["r"]);
    assert_eq!(names(src, "//x/ancestor-or-self::*[1]"), ["x"]);
    assert_eq!(names(src, "//x/ancestor::*[position() < 3]"), ["b", "c"]);

    let sibs = "<r><s1/><s2/><s3/><t/></r>";
    assert_eq!(names(sibs, "//t/preceding-sibling::*[1]"), ["s3"]);
    assert_eq!(names(sibs, "//t/preceding-sibling::*[last()]"), ["s1"]);
    assert_eq!(names(sibs, "//t/preceding::*[1]"), ["s3"]);
    assert_eq!(names(sibs, "//t/preceding::*[3]"), ["s1"]);
    // Forward axes are unchanged: `[1]` is the first in document order.
    assert_eq!(names(sibs, "//s1/following-sibling::*[1]"), ["s2"]);
    assert_eq!(names(sibs, "//s1/following::*[last()]"), ["t"]);
    // Whatever the axis, the result comes back in document order.
    assert_eq!(names(sibs, "//t/preceding-sibling::*"), ["s1", "s2", "s3"]);
}

/// Section 3.4: a comparison involving a node-set is true if there is
/// *some* node for which it holds, and two node-sets compare by any
/// pair. That applies to `<` as much as to `=`.
#[test]
fn relational_operators_are_existential_over_node_sets() {
    let src = "<r><a>1</a><a>9</a></r>";
    assert_eq!(val(src, "//a > 5"), "true");
    assert_eq!(val(src, "//a < 5"), "true");
    assert_eq!(val(src, "//a >= 9"), "true");
    assert_eq!(val(src, "//a <= 0"), "false");
    assert_eq!(val(src, "5 < //a"), "true");
    assert_eq!(val(src, "10 < //a"), "false");

    let outer = "<r><o><i>1</i><i>9</i></o><o><i>2</i></o></r>";
    assert_eq!(val(outer, "count(//o[i > 5])"), "1");
    // Any pair: 1 < 9 holds, so a set is `<` itself.
    assert_eq!(val(outer, "//o/i > //o/i"), "true");
    assert_eq!(val(outer, "//o/i < //o/i"), "true");
    // Against a string, both sides convert to numbers.
    assert_eq!(val(src, "//a > '5'"), "true");
    assert_eq!(val(src, "'5' > //a"), "true");
    // An empty set satisfies nothing.
    assert_eq!(val(src, "//missing < 5"), "false");
    assert_eq!(val(src, "//missing >= //a"), "false");
}

/// `!=` has its own existential reading: `//a != 'x'` asks whether
/// *some* node differs, which is not the negation of whether some
/// node is equal.
#[test]
fn inequality_is_existential_not_the_negation_of_equality() {
    let src = "<r><a>x</a><a>y</a></r>";
    assert_eq!(val(src, "//a != 'x'"), "true");
    assert_eq!(val(src, "not(//a = 'x')"), "false");
    assert_eq!(val(src, "//a = 'x'"), "true");
    // With every node equal, nothing differs.
    let same = "<r><a>x</a><a>x</a></r>";
    assert_eq!(val(same, "//a != 'x'"), "false");
    // Two node-sets: some pair differs.
    assert_eq!(val(src, "//a != //a"), "true");
    assert_eq!(val(same, "//a != //a"), "false");
    // An empty set is neither equal nor unequal to anything.
    assert_eq!(val(src, "//missing != 'x'"), "false");
    assert_eq!(val(src, "//missing = 'x'"), "false");
    // A boolean compares against the set's truth value.
    assert_eq!(val(src, "//a != true()"), "false");
    assert_eq!(val(src, "//missing != true()"), "true");
}

/// Section 3.3: a `FilterExpr` applies predicates to a primary
/// expression's node-set, counted in document order, and may continue
/// as a path.
#[test]
fn filter_expressions_take_predicates_and_continue_as_paths() {
    assert_eq!(texts(NESTED, "(//a)[1]"), ["1"]);
    assert_eq!(texts(NESTED, "(//a)[last()]"), ["4"]);
    assert_eq!(texts(NESTED, "(//a)[position() > 2]"), ["3", "4"]);
    assert_eq!(texts(NESTED, "(/r)//a"), ["1", "2", "3", "4"]);
    assert_eq!(texts(NESTED, "(//p)[2]/a"), ["3", "4"]);
    assert_eq!(texts(NESTED, "(//p)[2]/a[1]"), ["3"]);
    assert_eq!(texts(NESTED, "./r/p/a | (//a)[2]"), ["1", "2", "3", "4"]);
    assert_eq!(texts(NESTED, "(//a)[2] | (//a)[3]"), ["2", "3"]);
    // A function call is a primary expression too.
    let ids = "\
<!DOCTYPE r [<!ELEMENT r ANY><!ELEMENT a ANY><!ELEMENT b ANY>\
<!ATTLIST a key ID #IMPLIED>]>\
<r><a key=\"x\"><b>in x</b></a><a key=\"y\"><b>in y</b></a></r>";
    assert_eq!(texts(ids, "id('x')/b"), ["in x"]);
    assert_eq!(texts(ids, "id('x y')[2]/b"), ["in y"]);
    assert_eq!(texts(ids, "id('x')//b"), ["in x"]);
    // Predicates and steps still count against the operator budget.
    let mut limits = oxml::Limits::default();
    limits.max_xpath_operators = 3;
    assert!(XPath::compile_with_limits("(//a)[1]", &limits).is_ok());
    assert!(XPath::compile_with_limits("(//a)[1]/b", &limits).is_err());
}

/// A variable reference is a primary expression bound at evaluation.
#[test]
fn variables_are_bound_through_an_eval_context() {
    let doc = parse(NESTED).expect("well-formed");
    let xp = XPath::compile("//a[. = $want]").expect("compiles");
    assert_eq!(xp.variables(), [ExpandedName::local("want")]);

    let ctx = EvalContext::new()
        .bind(ExpandedName::local("want"), Value::String("3".into()));
    let value = xp.evaluate_with(&doc, &ctx).expect("bound");
    let found: Vec<String> = value
        .nodes()
        .unwrap()
        .iter()
        .map(|n| doc.text(*n))
        .collect();
    assert_eq!(found, ["3"]);

    // A number, a boolean and a node-set bind too, and a node-set
    // variable takes predicates and steps like any primary.
    let nodes = XPath::compile("//p").expect("compiles").evaluate(&doc);
    let ctx = EvalContext::new()
        .bind(ExpandedName::local("n"), Value::Number(2.0))
        .bind(ExpandedName::local("yes"), Value::Boolean(true))
        .bind(ExpandedName::local("ps"), nodes);
    let eval = |e: &str| {
        XPath::compile(e)
            .expect("compiles")
            .evaluate_with(&doc, &ctx)
            .expect("bound")
            .to_str(&doc)
    };
    assert_eq!(eval("$n + 1"), "3");
    assert_eq!(eval("$yes and true()"), "true");
    assert_eq!(eval("count($ps)"), "2");
    assert_eq!(eval("string($ps[2]/a[1])"), "3");
    assert_eq!(eval("string($ps[$n]/a[$n])"), "4");

    // A prefixed variable name resolves against the expression's
    // bindings, as a name test does.
    let xp = XPath::compile_with_namespaces("$m:v", &[("m", "urn:m")])
        .expect("compiles");
    assert_eq!(xp.variables(), [ExpandedName::qualified("urn:m", "v")]);
    assert!(XPath::compile("$q:v").is_err(), "an unbound prefix");
    assert!(XPath::compile("$").is_err(), "a name is required");
}

#[test]
fn an_unbound_variable_is_an_error_in_evaluate_with_and_empty_in_evaluate() {
    let doc = parse(NESTED).expect("well-formed");
    let xp = XPath::compile("//a[. = $want]").expect("compiles");
    let err = xp.evaluate_with(&doc, &EvalContext::new()).unwrap_err();
    assert!(matches!(err, EvalError::UnboundVariable(ref n) if n == "want"));
    assert!(err.to_string().contains("want"), "{err}");
    // The infallible entry point documents this: an expression with
    // variables evaluates to an empty node-set.
    assert_eq!(xp.evaluate(&doc), Value::NodeSet(Vec::new()));
    // Without variables, both entry points agree.
    let plain = XPath::compile("count(//a)").expect("compiles");
    assert!(plain.variables().is_empty());
    assert_eq!(
        plain.evaluate_with(&doc, &EvalContext::new()).expect("ok"),
        plain.evaluate(&doc)
    );
}

#[test]
fn a_stale_context_node_is_an_error_in_evaluate_with() {
    let mut doc = parse("<r><a/></r>").expect("well-formed");
    let a = XPath::compile("//a").expect("compiles").evaluate(&doc);
    let a = a.nodes().unwrap()[0];
    doc.remove(a).expect("removable");
    let xp = XPath::compile("count(.)").expect("compiles");
    let err = xp
        .evaluate_from_with(&doc, a, &EvalContext::new())
        .unwrap_err();
    assert_eq!(err, EvalError::Stale);
    let fresh = XPath::compile("//a").expect("compiles").evaluate(&doc);
    assert_eq!(fresh.nodes().unwrap().len(), 0);
}

/// Section 4.4: `sum` converts each node's string-value with
/// `number()`, so one non-numeric node makes the whole sum `NaN`.
#[test]
fn sum_is_nan_when_any_node_is_not_a_number() {
    let src = "<r><a>1</a><a>x</a><a>2</a></r>";
    assert_eq!(val(src, "sum(//a)"), "NaN");
    assert_eq!(val(src, "sum(//a[. != 'x'])"), "3");
    assert_eq!(val(src, "sum(//missing)"), "0");
}

/// Section 4.4: `number()` accepts exactly the `Number` production,
/// optionally negated and surrounded by XML whitespace. No exponent,
/// no plus sign, no `Infinity`, and no non-XML whitespace.
#[test]
fn number_follows_the_xpath_number_grammar_only() {
    let src = "<r/>";
    for (expr, want) in [
        ("number('1e5')", "NaN"),
        ("number('+1')", "NaN"),
        ("number('Infinity')", "NaN"),
        ("number('inf')", "NaN"),
        ("number('nan')", "NaN"),
        ("number('0x10')", "NaN"),
        ("number('1_000')", "NaN"),
        ("number('')", "NaN"),
        ("number('.')", "NaN"),
        ("number('-')", "NaN"),
        ("number('- 1')", "NaN"),
        ("number('1 2')", "NaN"),
        ("number('\u{a0}5')", "NaN"),
        ("number('5\u{a0}')", "NaN"),
        ("number('.5')", "0.5"),
        ("number('5.')", "5"),
        ("number('-.5')", "-0.5"),
        ("number('-5.')", "-5"),
        ("number(' 12 ')", "12"),
        ("number('\t\r\n12\n')", "12"),
        ("number('007')", "7"),
        ("number('-0')", "0"),
    ] {
        assert_eq!(val(src, expr), want, "{expr}");
    }
    // Every string-to-number path shares the grammar: comparisons,
    // arithmetic, predicates and `sum`.
    let doc = "<r><a>1e5</a><a> 7 </a></r>";
    assert_eq!(val(doc, "//a = 100000"), "false");
    assert_eq!(val(doc, "//a = 7"), "true");
    assert_eq!(val(doc, "sum(//a)"), "NaN");
    assert_eq!(val(doc, "'1e5' + 1"), "NaN");
    assert_eq!(val(doc, "' 7 ' + 1"), "8");
    assert_eq!(val(doc, "count(//a[. > 6])"), "1");
}

/// Section 3.4: numbers compare by IEEE 754 equality, which is exact.
/// An epsilon makes distinct numbers equal and infinity unequal to
/// itself.
#[test]
fn numeric_equality_is_ieee_not_epsilon() {
    let src = "<r><a/><a/></r>";
    assert_eq!(val(src, "1 div 0 = 1 div 0"), "true");
    assert_eq!(val(src, "(1 div 0) != (1 div 0)"), "false");
    // The grammar has no exponent form, so the powers of ten are
    // spelled out.
    let e17 = "100000000000000000";
    let e19 = "10000000000000000000";
    assert_eq!(val(src, &format!("1 div {e17} = 2 div {e17}")), "false");
    assert_eq!(val(src, &format!("0 = 1 div {e19}")), "false");
    assert_eq!(val(src, "0 div 0 = 0 div 0"), "false", "NaN is not NaN");
    assert_eq!(val(src, "0 div 0 != 0 div 0"), "true");
    assert_eq!(val(src, "-0 = 0"), "true");
    // Positional tests too: position 1 is not 1 + 1e-15, the nearest
    // distinct double being 1 + 2.2e-16.
    let e15 = "1000000000000000";
    assert_eq!(
        val(src, &format!("count(//a[position() = 1 + 1 div {e15}])")),
        "0"
    );
    assert_eq!(val(src, "count(//a[position() = 1])"), "1");
    assert_eq!(val(src, &format!("count(//a[1 + 1 div {e15}])")), "0");
    // Node-set against number uses the same comparison.
    let nums = "<r><a>0.3</a></r>";
    assert_eq!(val(nums, "//a = 0.1 + 0.2"), "false", "0.1 + 0.2 != 0.3");
    assert_eq!(val(nums, "//a = 0.3"), "true");
}

/// Section 5.4: only elements have namespace nodes.
#[test]
fn the_namespace_axis_from_a_non_element_is_empty() {
    let src = "<r xmlns:m=\"urn:m\" a=\"1\">text<!--c--><?pi d?></r>";
    assert_eq!(val(src, "count(/r/namespace::*)"), "2", "m and xml");
    assert_eq!(val(src, "count(/r/text()/namespace::*)"), "0");
    assert_eq!(val(src, "count(/r/comment()/namespace::*)"), "0");
    assert_eq!(
        val(src, "count(/r/processing-instruction()/namespace::*)"),
        "0"
    );
    assert_eq!(val(src, "count(/r/@a/namespace::*)"), "0");
    assert_eq!(val(src, "count(/r/namespace::*/namespace::*)"), "0");
    assert_eq!(val(src, "count(/namespace::*)"), "0");
}

/// Section 4.4: `round` is the closest integer, ties towards positive
/// infinity, and a negative argument that rounds to zero gives
/// negative zero. `floor(n + 0.5)` gets the first and last of these
/// wrong, and breaks on the largest odd integers.
#[test]
fn round_follows_the_specification_at_the_edges() {
    let src = "<r/>";
    // 0.49999999999999994 + 0.5 rounds to 1.0 in binary floating point.
    assert_eq!(val(src, "round(0.49999999999999994)"), "0");
    assert_eq!(val(src, "round(0.5)"), "1");
    assert_eq!(val(src, "round(1.5)"), "2");
    assert_eq!(val(src, "round(2.5)"), "3");
    assert_eq!(val(src, "round(-0.5)"), "0");
    assert_eq!(val(src, "round(-1.5)"), "-1");
    assert_eq!(val(src, "round(-2.5)"), "-2");
    assert_eq!(val(src, "round(-2.51)"), "-3");
    // Negative zero is only observable through division.
    assert_eq!(val(src, "1 div round(-0.4)"), "-Infinity");
    assert_eq!(val(src, "1 div round(-0.5)"), "-Infinity");
    assert_eq!(val(src, "1 div round(-0)"), "-Infinity");
    assert_eq!(val(src, "1 div round(0)"), "Infinity");
    assert_eq!(val(src, "1 div round(0.4)"), "Infinity");
    // 2^52 + 1: adding 0.5 is not representable and rounds away.
    assert_eq!(val(src, "round(4503599627370497)"), "4503599627370497");
    assert_eq!(val(src, "round(-4503599627370497)"), "-4503599627370497");
    assert_eq!(val(src, "round(1 div 0)"), "Infinity");
    assert_eq!(val(src, "round(-1 div 0)"), "-Infinity");
    assert_eq!(val(src, "round(0 div 0)"), "NaN");
}

/// Sections 4.3 and 4.1: whitespace, for `normalize-space` and the
/// token list `id` takes, is the XML `S` production: space, tab,
/// carriage return and line feed. A no-break space is a character
/// like any other.
#[test]
fn normalize_space_and_id_split_only_on_xml_whitespace() {
    let src = "<r/>";
    assert_eq!(
        val(src, "normalize-space(' a\u{a0}b \t\r\n c ')"),
        "a\u{a0}b c"
    );
    assert_eq!(val(src, "normalize-space('\u{a0}')"), "\u{a0}");
    assert_eq!(val(src, "string-length(normalize-space('\u{2003}x'))"), "2");

    let ids = "\
<!DOCTYPE r [<!ELEMENT r ANY><!ELEMENT a ANY><!ATTLIST a key ID #IMPLIED>]>\
<r><a key=\"p\">1</a><a key=\"q\">2</a></r>";
    assert_eq!(val(ids, "count(id('p\tq'))"), "2");
    assert_eq!(
        val(ids, "count(id('p\u{a0}q'))"),
        "0",
        "one token, no such id"
    );
}

/// Node-set functions given something else: a type error through the
/// fallible entry point, and the documented silent value through the
/// infallible one.
#[test]
fn node_set_functions_on_a_non_node_set_are_a_type_error_in_strict_mode() {
    let doc = parse("<r><a>1</a></r>").expect("well-formed");
    let ctx = EvalContext::new();
    for expr in [
        "count('x')",
        "count(1)",
        "sum('x')",
        "sum(true())",
        "local-name('x')",
        "name(1)",
        "namespace-uri(true())",
        "count(//a[count(1)])",
        "(1)[1]",
        "'s'/a",
    ] {
        let xp = XPath::compile(expr).expect("compiles");
        let err = xp.evaluate_with(&doc, &ctx).expect_err(expr);
        assert!(matches!(err, EvalError::TypeError(_)), "{expr}: {err}");
        // The infallible entry point still answers, as it always has.
        let _ = xp.evaluate(&doc);
    }
    // `id()` accepts anything: a non-node-set is converted to a string
    // and tokenised, by specification.
    let xp = XPath::compile("count(id('x'))").expect("compiles");
    assert_eq!(xp.evaluate_with(&doc, &ctx).expect("ok").to_str(&doc), "0");
    // Correct types pass through the fallible entry point unchanged.
    let xp = XPath::compile("count(//a) + sum(//a)").expect("compiles");
    assert_eq!(xp.evaluate_with(&doc, &ctx).expect("ok").to_str(&doc), "2");
    // The silent values are documented and must not change.
    let lenient = |e: &str| {
        XPath::compile(e)
            .expect("compiles")
            .evaluate(&doc)
            .to_str(&doc)
    };
    assert_eq!(lenient("count('x')"), "0");
    assert_eq!(lenient("sum('x')"), "0");
    assert_eq!(lenient("local-name('x')"), "");
    assert_eq!(lenient("count((1)[1])"), "0");
}

/// Both axes, defined as tree walks, must agree with their definitions
/// on a document where every relationship occurs: siblings before and
/// after, descendants of each, and ancestors.
#[test]
fn following_and_preceding_walk_the_tree() {
    let src = "<r><a><a1/><a2><a21/></a2></a>\
<b><b1/><b2><b21/><b22/></b2><b3/></b>\
<c><c1/></c></r>";
    assert_eq!(
        names(src, "//b2/following::*"),
        ["b3", "c", "c1"],
        "after b2, not its descendants"
    );
    assert_eq!(
        names(src, "//b2/preceding::*"),
        ["a", "a1", "a2", "a21", "b1"],
        "before b2, not its ancestors"
    );
    assert_eq!(names(src, "//b21/following::*"), ["b22", "b3", "c", "c1"]);
    assert_eq!(
        names(src, "//b22/preceding::*"),
        ["a", "a1", "a2", "a21", "b1", "b21"]
    );
    assert_eq!(names(src, "//c1/following::*"), Vec::<String>::new());
    assert_eq!(names(src, "//a1/preceding::*"), Vec::<String>::new());
    assert_eq!(names(src, "/r/following::*"), Vec::<String>::new());
    assert_eq!(names(src, "/r/preceding::*"), Vec::<String>::new());
    assert_eq!(names(src, "/following::*"), Vec::<String>::new());
    // Nearest first on the reverse axis; the forward axis in order.
    assert_eq!(names(src, "//b22/preceding::*[1]"), ["b21"]);
    assert_eq!(names(src, "//b22/preceding::*[2]"), ["b1"]);
    assert_eq!(names(src, "//b21/following::*[1]"), ["b22"]);
    // An attribute's following is its element's content and beyond;
    // its preceding is its element's preceding.
    let attrs = "<r><a x=\"1\"><a1/></a><b y=\"2\"/></r>";
    assert_eq!(names(attrs, "//a/@x/following::*"), ["a1", "b"]);
    assert_eq!(names(attrs, "//b/@y/preceding::*"), ["a", "a1"]);
    assert_eq!(val(attrs, "count(//b/@y/following::node())"), "0");
    // Every node in a 3,001-node document, counted both ways, agrees
    // with the closed form: each of the n elements under the root
    // follows every element before it, so the step visits
    // n(n - 1)/2 candidates before the union collapses them.
    let big = generate(3_000);
    assert_eq!(val(&big, "count(//*/following::*)"), "2999");
    assert_eq!(val(&big, "count(//*/preceding::*)"), "2999");
    assert_eq!(val(&big, "count(//e[last()]/preceding::*)"), "2999");
}

/// `n` elements under a root: `<r><e/><e/>...</r>`.
fn generate(n: usize) -> String {
    let mut s = String::with_capacity(n * 4 + 8);
    s.push_str("<r>");
    for _ in 0..n {
        s.push_str("<e/>");
    }
    s.push_str("</r>");
    s
}

/// A built document can be deeper than any parsed one, since
/// `Limits::max_depth` only applies to parsing. The descendant walk
/// must not recurse per level.
#[test]
fn a_deep_built_document_does_not_overflow_the_descendant_walk() {
    let doc = std::thread::Builder::new()
        .stack_size(512 * 1024)
        .spawn(|| {
            let mut doc = Document::empty();
            let mut at = doc.append_element(doc.root(), None, "d").unwrap();
            for _ in 0..50_000 {
                at = doc.append_element(at, None, "d").unwrap();
            }
            let count = XPath::compile("count(//d)").unwrap().evaluate(&doc);
            assert_eq!(count.to_str(&doc), "50001");
            let last = XPath::compile("count(//d[not(d)]/ancestor::d)")
                .unwrap()
                .evaluate(&doc);
            assert_eq!(last.to_str(&doc), "50000");
            let following = XPath::compile("count(/d/following::*)")
                .unwrap()
                .evaluate(&doc);
            assert_eq!(following.to_str(&doc), "0");
            doc
        })
        .expect("spawn")
        .join()
        .expect("no overflow");
    assert_eq!(doc.len(), 50_002);
}

/// An unknown function is reported where its name starts, so an
/// editor can underline the name rather than the closing parenthesis.
#[test]
fn an_unknown_function_is_reported_at_its_name() {
    let err = XPath::compile("1 + frobnicate(1, 2)").unwrap_err();
    assert_eq!(err.offset, 4, "{err}");
    assert!(err.message.contains("unknown function"), "{err}");
    let err = XPath::compile("//a[  nope()]").unwrap_err();
    assert_eq!(err.offset, 6, "{err}");
    // The arity check is reported at the name as well.
    let err = XPath::compile("1 + count()").unwrap_err();
    assert_eq!(err.offset, 4, "{err}");
    assert!(err.message.contains("wrong number"), "{err}");
}

/// `translate` builds its map once; semantics are unchanged by that.
#[test]
fn translate_keeps_the_first_occurrence_and_deletes_unpaired() {
    let src = "<r/>";
    assert_eq!(val(src, "translate('abcabc', 'abc', 'AB')"), "ABAB");
    assert_eq!(val(src, "translate('aaa', 'aa', 'xy')"), "xxx");
    assert_eq!(val(src, "translate('', 'a', 'b')"), "");
    assert_eq!(val(src, "translate('abc', '', 'xyz')"), "abc");
    assert_eq!(val(src, "translate('héllo', 'é', 'e')"), "hello");
}

/// Node-set equality by hoisting the smaller set's values: the answer
/// cannot depend on which side is smaller.
#[test]
fn node_set_equality_is_symmetric_whatever_the_sizes() {
    let src = "<r><a>1</a><a>2</a><a>3</a><b>3</b><c>9</c></r>";
    assert_eq!(val(src, "//a = //b"), "true");
    assert_eq!(val(src, "//b = //a"), "true");
    assert_eq!(val(src, "//a = //c"), "false");
    assert_eq!(val(src, "//c = //a"), "false");
    assert_eq!(val(src, "//a != //b"), "true");
    assert_eq!(val(src, "//b != //a"), "true");
    assert_eq!(val(src, "//b != //b"), "false");
    assert_eq!(val(src, "//missing = //a"), "false");
    assert_eq!(val(src, "//a = //missing"), "false");
}

/// A document mutated through the tree API: a removed slot reused by
/// a later node, and a node moved under a new parent. Arena order is
/// no longer document order, so every node-set must come back in the
/// order the document serialises in, and the axes that are defined
/// by document order must agree with it.
#[test]
fn a_mutated_document_still_evaluates_in_document_order() {
    let mut doc = parse("<top><a>1</a><a>2</a><a>3</a></top>").expect("ok");
    let top = doc.root_element().expect("top");
    let second = doc.children(top)[1];
    doc.remove(second).expect("removable");
    // The slot `a 2` held is reused for a node that is *last* in
    // document order.
    let fourth = doc.append_element(top, None, "a").expect("appends");
    let _ = doc.append_text(fourth, "4").expect("appends");
    assert!(doc.is_order_dirty(), "a mutation marks the order stale");
    assert_eq!(doc.to_xml(), "<top><a>1</a><a>3</a><a>4</a></top>");

    assert_eq!(texts_of(&doc, "//a"), ["1", "3", "4"]);
    assert_eq!(val_of(&doc, "string(/top/*[1])"), "1");
    assert_eq!(val_of(&doc, "string(/top/*[last()])"), "4");
    assert_eq!(texts_of(&doc, "//a[.='3']/following::*"), ["4"]);
    assert_eq!(texts_of(&doc, "//a[.='4']/preceding::*"), ["1", "3"]);
    assert_eq!(
        texts_of(&doc, "//a[.='1']/following-sibling::*"),
        ["3", "4"]
    );
    assert_eq!(
        texts_of(&doc, "//a[.='4']/preceding-sibling::*"),
        ["1", "3"]
    );
    assert_eq!(texts_of(&doc, "(//a)[2]"), ["3"]);
    assert_eq!(texts_of(&doc, "//a[.='4'] | //a[.='1']"), ["1", "4"]);

    // Refreshing the document's own cache gives the same answers
    // without the per-evaluation walk.
    let _ = doc.document_order();
    assert!(!doc.is_order_dirty());
    assert_eq!(texts_of(&doc, "//a"), ["1", "3", "4"]);
    assert_eq!(texts_of(&doc, "//a[.='4']/preceding::*"), ["1", "3"]);

    // Moving the first `a` to the end changes every order-dependent
    // answer with it.
    let first = doc.children(top)[0];
    doc.reparent(first, top).expect("moves");
    assert_eq!(doc.to_xml(), "<top><a>3</a><a>4</a><a>1</a></top>");
    assert_eq!(texts_of(&doc, "//a"), ["3", "4", "1"]);
    assert_eq!(val_of(&doc, "string(/top/*[1])"), "3");
    assert_eq!(texts_of(&doc, "//a[.='4']/following::*"), ["1"]);
    assert_eq!(texts_of(&doc, "//a[.='1']/preceding::*"), ["3", "4"]);
    assert_eq!(
        texts_of(&doc, "//a[.='3']/following-sibling::*"),
        ["4", "1"]
    );
    assert_eq!(texts_of(&doc, "//a[.='1']/preceding-sibling::*[1]"), ["4"]);
    assert_eq!(texts_of(&doc, "(//a)[last()]"), ["1"]);
}

/// The text of every node `expr` selects from an existing document.
fn texts_of(doc: &Document, expr: &str) -> Vec<String> {
    let value = XPath::compile(expr).expect("compiles").evaluate(doc);
    value
        .nodes()
        .expect("set")
        .iter()
        .map(|n| doc.text(*n))
        .collect()
}

/// The string-value of `expr` against an existing document.
fn val_of(doc: &Document, expr: &str) -> String {
    XPath::compile(expr)
        .expect("compiles")
        .evaluate(doc)
        .to_str(doc)
}

/// The one-walk union of `following::` and `preceding::` over many
/// context nodes must agree with the per-node definition, which a
/// predicate forces the evaluator to use.
#[test]
fn the_axis_unions_agree_with_the_per_node_definition() {
    let src = "<r a=\"1\"><a x=\"1\"><a1/><a2><a21/>t<!--c--></a2></a>\
<b y=\"2\"><b1/><b2><b21/><b22/></b2><b3/></b>\
<c><c1><?pi d?></c1></c></r>";
    let doc = parse(src).expect("well-formed");
    for context in [
        "//*",
        "//@*",
        "//node()",
        "//a2 | //b",
        "//*[2]",
        "/",
        "//text()",
    ] {
        for axis in ["following", "preceding"] {
            for test in ["*", "node()", "text()", "b2"] {
                let closed = format!("{context}/{axis}::{test}");
                let per_node = format!("{context}/{axis}::{test}[true()]");
                let a =
                    XPath::compile(&closed).expect("compiles").evaluate(&doc);
                let b =
                    XPath::compile(&per_node).expect("compiles").evaluate(&doc);
                assert_eq!(a, b, "`{closed}` against `{per_node}`");
            }
        }
    }
}
