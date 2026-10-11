// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! `XPath` expressions are untrusted input, and `Limits` bounds them.
//!
//! Every case here runs on a thread with a 2 MiB stack, smaller than
//! the main thread's 8 MiB, so a recursion that scales with the
//! expression's length shows up as an abort here before it does in a
//! front end. The parser must return an error for every one of these
//! shapes; it must never run out of stack in compile, evaluate, or
//! drop.
//!
//! The default `max_xpath_depth` of 256 has to fit on that stack in a
//! debug build before the limit can refuse level 257. Measured by
//! bisecting the thread's stack size (debug profile, aarch64 macOS,
//! rustc 1.98): 256 nested parentheses need about 1032 KiB, 256
//! nested `not(` about 1254 KiB, and 256 nested predicates, the most
//! expensive shape at eight parser frames per level, about 1508 KiB.
//! A 10_000-term flat chain needs 64 KiB, because a chain is a `Vec`
//! and not a tree. If a toolchain change makes a test here abort,
//! the per-level frame cost has grown; the fix is in the parser, not
//! in this file's stack size. The size of `Expr` is part of that
//! cost, which is why its large variants are boxed.

#![cfg(all(feature = "xpath", feature = "std"))]

use oxml::{Document, Limits, XPath, XPathError};

const STACK: usize = 2 * 1024 * 1024;

fn on_small_stack<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> T {
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(f)
        .expect("spawn")
        .join()
        .expect("the thread must not panic or abort")
}

fn doc() -> Document {
    oxml::parse("<r><x/><x/></r>").expect("well-formed")
}

/// Compile, evaluate and drop, reporting only whether compile failed.
fn compile_eval_drop(expr: &str) -> Result<(), XPathError> {
    let compiled = XPath::compile(expr)?;
    let d = doc();
    let value = compiled.evaluate(&d);
    drop(value);
    drop(compiled);
    Ok(())
}

#[test]
fn a_million_unary_minuses_is_an_error_not_an_abort() {
    let expr = "-".repeat(1_000_000) + "1";
    let r = on_small_stack(move || compile_eval_drop(&expr));
    assert!(r.is_err(), "rejected by the operator budget");
}

#[test]
fn a_million_term_addition_is_an_error_not_an_abort() {
    let expr = "1+".repeat(1_000_000) + "1";
    let r = on_small_stack(move || compile_eval_drop(&expr));
    assert!(r.is_err());
}

#[test]
fn a_million_term_or_chain_is_an_error_not_an_abort() {
    let expr = "1 or ".repeat(1_000_000) + "1";
    let r = on_small_stack(move || compile_eval_drop(&expr));
    assert!(r.is_err());
}

#[test]
fn a_million_term_union_is_an_error_not_an_abort() {
    let expr = "//x|".repeat(1_000_000) + "//x";
    let r = on_small_stack(move || compile_eval_drop(&expr));
    assert!(r.is_err());
}

#[test]
fn a_hundred_thousand_parentheses_is_an_error_not_an_abort() {
    let expr = "(".repeat(100_000) + "1" + &")".repeat(100_000);
    let r = on_small_stack(move || compile_eval_drop(&expr));
    let e = r.expect_err("beyond the depth limit");
    assert!(e.to_string().contains("deep"), "{e}");
}

#[test]
fn a_hundred_thousand_nested_calls_is_an_error_not_an_abort() {
    let expr = "not(".repeat(100_000) + "1" + &")".repeat(100_000);
    let r = on_small_stack(move || compile_eval_drop(&expr));
    let e = r.expect_err("beyond the depth limit");
    assert!(e.to_string().contains("deep"), "{e}");
}

#[test]
fn nested_predicates_count_towards_depth() {
    // `//x[.[.[.[1]]]]`: each predicate holds a step with another.
    let expr =
        "//x".to_owned() + &"[.".repeat(100_000) + "[1]" + &"]".repeat(100_000);
    let r = on_small_stack(move || compile_eval_drop(&expr));
    let e = r.expect_err("beyond the depth limit");
    assert!(e.to_string().contains("deep"), "{e}");
}

#[test]
fn strict_limits_bound_the_operator_count() {
    let strict = Limits::strict();
    assert_eq!(
        strict.max_xpath_operators, 1_000,
        "the fixture assumes this"
    );
    let under = "1+".repeat(999) + "1";
    assert!(XPath::compile_with_limits(&under, &strict).is_ok(), "999");
    let over = "1+".repeat(1_001) + "1";
    let e =
        XPath::compile_with_limits(&over, &strict).expect_err("1001 operators");
    assert!(e.to_string().contains("operator"), "{e}");
    // The same text compiles under the default budget.
    assert!(XPath::compile(&over).is_ok(), "10_000 is the default");
}

#[test]
fn strict_limits_bound_the_nesting_depth() {
    let strict = Limits::strict();
    assert_eq!(strict.max_xpath_depth, 32, "the fixture assumes this");
    let under = "(".repeat(31) + "1" + &")".repeat(31);
    assert!(XPath::compile_with_limits(&under, &strict).is_ok(), "31");
    let over = "(".repeat(33) + "1" + &")".repeat(33);
    let e = XPath::compile_with_limits(&over, &strict).expect_err("depth 33");
    assert!(e.to_string().contains("deep"), "{e}");
    assert!(XPath::compile(&over).is_ok(), "256 is the default");
}

#[test]
fn namespaces_and_limits_combine() {
    let strict = Limits::strict();
    let x = XPath::compile_with_namespaces_and_limits(
        "//m:a",
        &[("m", "urn:u")],
        &strict,
    )
    .expect("bound prefix, within limits");
    let d = oxml::parse(r#"<r xmlns:m="urn:u"><m:a>yes</m:a></r>"#).unwrap();
    assert_eq!(x.evaluate(&d).to_str(&d), "yes");
    assert!(
        XPath::compile_with_namespaces_and_limits("//q:a", &[], &strict)
            .is_err(),
        "an unbound prefix is still an error"
    );
    let over = "1+".repeat(1_001) + "1";
    assert!(
        XPath::compile_with_namespaces_and_limits(&over, &[], &strict).is_err(),
        "the operator budget still applies"
    );
}

#[test]
fn a_long_chain_within_the_default_budget_evaluates_on_a_small_stack() {
    let expr = "1+".repeat(9_000) + "1";
    let n = on_small_stack(move || {
        let x = XPath::compile(&expr).expect("9_000 operators is under 10_000");
        let d = doc();
        x.evaluate(&d).to_number(&d)
    });
    assert!((n - 9_001.0).abs() < f64::EPSILON, "{n}");
}

#[test]
fn a_long_or_chain_short_circuits() {
    // `1 or` decides the whole chain at the first term; the trailing
    // `0 div 0` is NaN, which is false, so a chain that evaluated
    // every term and took the last would answer false.
    let expr = "1 or ".repeat(5_000) + "0 div 0";
    let v = on_small_stack(move || {
        let x = XPath::compile(&expr).expect("5_000 operators");
        let d = doc();
        x.evaluate(&d).to_boolean()
    });
    assert!(v);
    // And the other way round: `0 and` decides false at once.
    let expr = "0 and ".repeat(5_000) + "1";
    let v = on_small_stack(move || {
        let x = XPath::compile(&expr).expect("5_000 operators");
        let d = doc();
        x.evaluate(&d).to_boolean()
    });
    assert!(!v);
}

#[test]
fn chain_semantics_match_the_nested_form() {
    // A chain is a left fold: `8 - 3 - 2` is `(8 - 3) - 2`, not
    // `8 - (3 - 2)`; `2 = 2 = 1` compares the boolean `true` against
    // `1`, which is true; `1 < 2 < 3` compares `true` (1) against 3.
    let d = doc();
    let eval = |e: &str| XPath::compile(e).expect("compiles").evaluate(&d);
    let num = |e: &str| -> i64 {
        i64::try_from(eval(e).to_number(&d) as i128).expect("integral")
    };
    assert_eq!(num("8 - 3 - 2"), 3);
    assert_eq!(num("100 div 10 div 5"), 2);
    assert_eq!(num("17 mod 10 mod 4"), 3);
    assert_eq!(num("2 * 3 + 4 * 5 - 6"), 20);
    assert!(eval("2 = 2 = 1").to_boolean());
    assert!(!eval("2 = 2 != 1").to_boolean());
    assert!(eval("1 < 2 < 3").to_boolean());
    assert!(!eval("3 > 2 > 1").to_boolean());
    assert_eq!(num("count(//x | //x | //r)"), 3);
    assert_eq!(num("1 - -1"), 2);
    assert_eq!(num("- - 1"), 1);
    assert_eq!(num("---1"), -1);
    assert!(eval("--'abc'").to_number(&d).is_nan());
    assert!(eval("1 and 1 and 0 or 1").to_boolean());
    assert!(!eval("1 and 0 or 0").to_boolean());
}

#[test]
fn a_chain_clones_compares_and_debugs_without_deep_recursion() {
    let expr = "1+".repeat(9_000) + "1";
    on_small_stack(move || {
        let x = XPath::compile(&expr).expect("compiles");
        let copy = x.clone();
        assert_eq!(x, copy);
        assert!(!format!("{:?}", x.expr()).is_empty());
        drop(copy);
        drop(x);
    });
}

#[test]
fn operators_steps_predicates_and_calls_all_count() {
    let mut one = Limits::default();
    one.max_xpath_operators = 3;
    let accepts = |e: &str| XPath::compile_with_limits(e, &one).is_ok();
    // A path of three steps is three operations: exactly the budget.
    assert!(accepts("/a/b/c"));
    assert!(!accepts("/a/b/c/d"));
    // A predicate costs one on top of its step.
    assert!(accepts("/a/b[1]"));
    assert!(!accepts("/a/b[1]/c"));
    // A function call costs one, plus whatever its arguments cost.
    assert!(accepts("not(1 + 1 + 1)"));
    assert!(!accepts("not(1 + 1 + 1 + 1)"));
    // A unary minus is an operator.
    assert!(accepts("---1"));
    assert!(!accepts("----1"));
    // `//` is a step of its own.
    assert!(accepts("//a/b"));
    assert!(!accepts("//a//b"));
    // Parentheses and literals are free.
    assert!(accepts("((((('x')))))"));
}
