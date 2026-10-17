// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `Display` renders a compiled expression as canonical unabbreviated
//! `XPath` 1.0, and compiling the rendering gives back the same tree.
//!
//! The corpus below is every shape the other `XPath` suites exercise;
//! each of those suites also round-trips every expression it compiles
//! through its helper, so the claim is checked on the way past rather
//! than only here.

#![cfg(feature = "xpath")]

use oxml::XPath;

const NAMESPACES: &[(&str, &str)] = &[("m", "urn:m"), ("q", "urn:u")];

/// Compiling what `Display` wrote, under the same bindings, must give
/// an equal `XPath`.
fn round_trips(expr: &str) -> String {
    let compiled = XPath::compile_with_namespaces(expr, NAMESPACES)
        .unwrap_or_else(|e| panic!("`{expr}` failed to compile: {e}"));
    let rendered = compiled.to_string();
    let again = XPath::compile_with_namespaces(&rendered, NAMESPACES)
        .unwrap_or_else(|e| panic!("`{expr}` rendered as `{rendered}`: {e}"));
    assert_eq!(again, compiled, "`{expr}` rendered as `{rendered}`");
    assert_eq!(again.to_string(), rendered, "rendering is a fixed point");
    rendered
}

#[test]
fn abbreviations_render_in_full() {
    assert_eq!(
        round_trips("//a[1]"),
        "/descendant-or-self::node()/child::a[1]"
    );
    assert_eq!(round_trips("."), "self::node()");
    assert_eq!(round_trips(".."), "parent::node()");
    assert_eq!(round_trips("@x"), "attribute::x");
    assert_eq!(round_trips("/"), "/");
    assert_eq!(
        round_trips("a/b//c"),
        "child::a/child::b/descendant-or-self::node()/child::c"
    );
    assert_eq!(
        round_trips("//@*"),
        "/descendant-or-self::node()/attribute::*"
    );
    assert_eq!(
        round_trips(".//b"),
        "self::node()/descendant-or-self::node()/child::b"
    );
}

#[test]
fn operators_are_parenthesised_by_level() {
    assert_eq!(round_trips("1 + 2 * 3"), "(1 + (2 * 3))");
    assert_eq!(round_trips("(1 + 2) * 3"), "((1 + 2) * 3)");
    assert_eq!(round_trips("1 + 2 + 3"), "(1 + 2 + 3)");
    assert_eq!(
        round_trips("a or b and c"),
        "(child::a or (child::b and child::c))"
    );
    assert_eq!(
        round_trips("//a | //b"),
        "(/descendant-or-self::node()/child::a | /descendant-or-self::node()/child::b)"
    );
    assert_eq!(round_trips("-1"), "-1");
    assert_eq!(round_trips("--'3'"), "--'3'");
    assert_eq!(round_trips("---1"), "-1");
    // A parenthesised negation under a pair keeps its parentheses: the
    // parser folds a run of `-` to one or two negations, so `---3.5`
    // would read back as a single one. Found by the xpath_compile
    // fuzz target.
    assert_eq!(round_trips("--(-3.5)"), "--(-3.5)");
    assert_eq!(round_trips("--(--'3')"), "--(--'3')");
    assert_eq!(round_trips("-(-1)"), "--1");
    // `-` binds more loosely than `|`, so a negated union operand keeps
    // its parentheses: `-1.5 | 2` reads back as `-(1.5 | 2)`.
    assert_eq!(round_trips("(-1.5) | 2"), "((-1.5) | 2)");
    assert_eq!(round_trips("2 | (-1) | 3"), "(2 | (-1) | 3)");
    assert_eq!(round_trips("-(1 | 2)"), "-(1 | 2)");
    assert_eq!(round_trips("1 div 2 mod 3"), "(1 div 2 mod 3)");
    assert_eq!(round_trips("1 < 2 <= 3 > 0 >= 0"), "(1 < 2 <= 3 > 0 >= 0)");
    assert_eq!(round_trips("1 = 1 != 0"), "(1 = 1 != 0)");
}

#[test]
fn root_operands_keep_their_token_boundaries() {
    assert_eq!(
        round_trips("omment*(/)*/*/comment()/*"),
        "(child::omment * (/) * /child::*/child::comment()/child::*)"
    );
    for op in [
        "or", "and", "=", "!=", "<", "<=", ">", ">=", "+", "-", "*", "div",
        "mod", "|",
    ] {
        for root in ["(/)", "(-/)", "(--/)", "(--(-/))"] {
            let _ = round_trips(&format!("{root} {op} 2"));
            let _ = round_trips(&format!("1 {op} {root} {op} /r"));
        }
    }
}

#[test]
fn literals_and_numbers_render_as_the_parser_reads_them() {
    assert_eq!(round_trips("'a'"), "'a'");
    assert_eq!(round_trips("\"it's\""), "\"it's\"");
    assert_eq!(round_trips("'say \"hi\"'"), "'say \"hi\"'");
    assert_eq!(round_trips("1.50"), "1.5");
    assert_eq!(round_trips(".5"), "0.5");
    assert_eq!(round_trips("5."), "5");
    assert_eq!(round_trips("0.1"), "0.1");
    assert_eq!(round_trips("1.23456789012345678"), "1.2345678901234567");
    assert_eq!(
        round_trips("1000000000000000000000"),
        "1000000000000000000000"
    );
}

#[test]
fn node_tests_functions_filters_and_variables_render() {
    assert_eq!(
        round_trips("//text()"),
        "/descendant-or-self::node()/child::text()"
    );
    assert_eq!(
        round_trips("//comment()"),
        "/descendant-or-self::node()/child::comment()"
    );
    assert_eq!(
        round_trips("//processing-instruction()"),
        "/descendant-or-self::node()/child::processing-instruction()"
    );
    assert_eq!(
        round_trips("//processing-instruction('render')"),
        "/descendant-or-self::node()/child::processing-instruction('render')"
    );
    assert_eq!(
        round_trips("//*:item"),
        "/descendant-or-self::node()/child::*:item"
    );
    assert_eq!(
        round_trips("//m:a/@q:b"),
        "/descendant-or-self::node()/child::m:a/attribute::q:b"
    );
    assert_eq!(
        round_trips("//@xml:lang"),
        "/descendant-or-self::node()/attribute::xml:lang"
    );
    assert_eq!(
        round_trips("concat('a', 1, true())"),
        "concat('a', 1, true())"
    );
    assert_eq!(
        round_trips("count(//a[@x = 'y'][2])"),
        "count(/descendant-or-self::node()/child::a[(attribute::x = 'y')][2])"
    );
    assert_eq!(
        round_trips("(//a)[1]"),
        "(/descendant-or-self::node()/child::a)[1]"
    );
    assert_eq!(round_trips("id('x')/b"), "(id('x'))/child::b");
    assert_eq!(round_trips("$v"), "$v");
    assert_eq!(
        round_trips("$m:v[1]//c"),
        "($m:v)[1]/descendant-or-self::node()/child::c"
    );
    for axis in oxml::xpath::Axis::ALL {
        let expr = format!("//a/{}::node()", axis.name());
        let want = format!(
            "/descendant-or-self::node()/child::a/{}::node()",
            axis.name()
        );
        assert_eq!(round_trips(&expr), want);
    }
    for func in oxml::xpath::Func::ALL {
        let (min, _) = func.arity();
        let args = vec!["1"; min].join(", ");
        let expr = format!("{}({args})", func.name());
        assert_eq!(round_trips(&expr), expr);
    }
}

/// Without bindings an `Expr` has no prefix to write and falls back
/// to Clark notation; the `XPath` it came from writes the prefix.
#[test]
fn an_expr_without_bindings_writes_clark_notation() {
    let xp = XPath::compile_with_namespaces("//m:a", NAMESPACES).unwrap();
    assert_eq!(xp.to_string(), "/descendant-or-self::node()/child::m:a");
    assert_eq!(
        xp.expr().to_string(),
        "/descendant-or-self::node()/child::{urn:m}a"
    );
    let plain = XPath::compile("//a[. = $v]").unwrap();
    assert_eq!(plain.expr().to_string(), plain.to_string());
}

/// A literal built by hand that holds both quotes, which no parsed
/// expression can, is written as a `concat` that evaluates to it.
#[test]
fn a_literal_with_both_quotes_is_written_as_concat() {
    let expr = oxml::xpath::Expr::Literal("a'b\"c".into());
    let rendered = expr.to_string();
    assert_eq!(rendered, "concat('a', \"'\", 'b\"c', '')");
    let doc = oxml::parse("<r/>").unwrap();
    let value = XPath::compile(&rendered).unwrap().evaluate(&doc);
    assert_eq!(value.to_str(&doc), "a'b\"c");
}
