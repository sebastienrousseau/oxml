// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The four `XPath` value types, and evaluating from a context node.
//!
//! Run with:
//!
//! ```text
//! cargo run --example xpath_values
//! ```

use oxml::xpath::{Axis, BinaryOp, EvalContext, EvalLimits, Func, Value};
use oxml::{ExpandedName, Limits, XPath, parse};

const DOC: &str = r#"<?xml version="1.0"?>
<shop>
  <item price="9.99" stock="3">Tea</item>
  <item price="4.50" stock="0">Coffee</item>
  <item price="12.00" stock="7">Cocoa</item>
</shop>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse(DOC)?;
    let bounded = EvalContext::new().with_limits(EvalLimits::default());
    assert_eq!(
        XPath::compile("count(//item)")?.evaluate_with(&doc, &bounded)?,
        Value::Number(3.0)
    );

    // An expression compiles once and is independent of any document,
    // so a server can compile at startup and evaluate per request.
    for expr in [
        "//item",             // node-set
        "string(//item[1])",  // string
        "sum(//item/@price)", // number
        "count(//item) > 2",  // boolean
    ] {
        let compiled = XPath::compile(expr)?;
        let value = compiled.evaluate(&doc);
        let kind = match value {
            Value::NodeSet(_) => "node-set",
            Value::String(_) => "string",
            Value::Number(_) => "number",
            Value::Boolean(_) => "boolean",
        };
        println!("{expr:<24} -> {kind}");
    }

    println!("\n== converting between them ==");
    // Every value converts to every other type, by XPath's own rules
    // rather than Rust's. A node-set is true when it is non-empty, and
    // its string-value is that of its *first* node in document order.
    let items = XPath::compile("//item")?.evaluate(&doc);
    println!("  to_boolean : {}", items.to_boolean());
    println!("  to_str     : {:?}", items.to_str(&doc));
    println!("  to_number  : {}", items.to_number(&doc));
    println!(
        "  nodes      : {} found",
        items.nodes().unwrap_or(&[]).len()
    );

    let empty = XPath::compile("//nothing")?.evaluate(&doc);
    println!("\n  empty node-set to_boolean: {}", empty.to_boolean());
    // Converting a non-numeric string gives NaN, not an error -- XPath
    // has no exceptions, so every conversion has to produce something,
    // and `NaN` is how "not a number" is spelled.
    let tea = XPath::compile("string(//item[1])")?.evaluate(&doc);
    println!(
        "  {:?} as a number:       {}",
        tea.to_str(&doc),
        tea.to_number(&doc)
    );

    let count = XPath::compile("count(//item)")?.evaluate(&doc);
    println!("\n  a number's to_str: {:?}", count.to_str(&doc));
    // A number is not a node-set, so there is nothing to iterate.
    println!("  a number's nodes:  {:?}", count.nodes());

    println!("\n== relative to a context node ==");
    // `evaluate` starts at the document root. `evaluate_from` starts
    // wherever you say, which is how you run a relative expression
    // against each match of an outer one.
    let rows = XPath::compile("//item")?;
    let price = XPath::compile("@price")?;
    let name = XPath::compile("string(.)")?;
    for &node in rows.evaluate(&doc).nodes().unwrap_or(&[]) {
        println!(
            "  {:<8} {}",
            name.evaluate_from(&doc, node).to_str(&doc),
            price.evaluate_from(&doc, node).to_number(&doc),
        );
    }

    println!("\n== namespaces ==");
    // A prefix in an expression resolves against the expression's own
    // bindings, never the document's declarations. The two can use
    // different prefixes for the same URI, and only the URI matters.
    let ns = parse(
        r#"<r xmlns:m="urn:u"><m:item>ns</m:item><item>plain</item></r>"#,
    )?;
    let bound = XPath::compile_with_namespaces("//q:item", &[("q", "urn:u")])?;
    println!(
        "  //q:item bound to urn:u -> {:?}",
        bound.evaluate(&ns).to_str(&ns)
    );

    // An unprefixed name test matches nodes in *no* namespace. This is
    // XPath 1.0's classic surprise: a default namespace does not apply
    // to node tests.
    let bare = XPath::compile("//item")?;
    println!(
        "  //item (no namespace)    -> {:?}",
        bare.evaluate(&ns).to_str(&ns)
    );

    // An unbound prefix is refused rather than quietly matching on the
    // local part, which is what this used to do.
    match XPath::compile("//m:item") {
        Ok(_) => println!("  unexpectedly compiled"),
        Err(e) => println!("  //m:item unbound         -> {e}"),
    }

    show_limits(&ns)?;
    show_variables(&doc)?;
    show_the_rendering()?;

    println!("\n== the compiled form ==");
    // `expr` exposes the parsed syntax tree, which is useful for
    // caching keys, static analysis, or simply showing what an
    // expression was understood to mean.
    let compiled = XPath::compile("//item[@stock > 0]/@price")?;
    println!("  {:?}", compiled.expr());

    // A malformed expression fails at compile time, not at evaluation.
    match XPath::compile("//item[") {
        Ok(_) => println!("\n  unexpectedly compiled"),
        Err(e) => println!("\n  rejected at compile time: {e}"),
    }

    // Typed extraction: name the Rust type, get that type or an error.
    // Inside an expression a non-number converts to NaN, as the
    // specification requires. At the boundary into Rust it is an
    // error instead, because a caller who names f64 wants a number --
    // NaN would poison every comparison downstream with no hint of
    // where it came from.
    let total: f64 = doc.xpath_one("sum(//item/@price)").expect("numeric");
    let count: i64 = doc.xpath_one("count(//item)").expect("integral");
    let any_out: bool = doc
        .xpath_one("count(//item[@stock = 0]) > 0")
        .expect("boolean");
    println!("\n  total {total}, {count} items, out of stock: {any_out}");

    let prices: Vec<f64> = doc.xpath_all("//item/@price").expect("all numeric");
    println!("  prices: {prices:?}");
    assert_eq!(prices.len() as i64, count);

    let not_a_number: Result<f64, _> = doc.xpath_one("string(//item[1])");
    println!("  `Tea` as f64: {not_a_number:?}");
    assert!(not_a_number.is_err(), "text must not extract as a number");
    Ok(())
}

/// `$name` is bound at evaluation, not in the expression, so one
/// compiled query serves many parameters. `evaluate` answers an empty
/// node-set for an expression with variables, since nothing binds
/// them there; `evaluate_with` takes the bindings and reports what the
/// infallible entry points answer silently: an unbound variable, or a
/// node-set function given something else.
fn show_variables(
    doc: &oxml::Document,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== variables ==");
    let cheap = XPath::compile("//item[@price < $max]")?;
    println!("  references: {:?}", cheap.variables());
    for max in [5.0, 10.0] {
        let ctx = EvalContext::new()
            .bind(ExpandedName::local("max"), Value::Number(max));
        let found = cheap.evaluate_with(doc, &ctx)?;
        println!(
            "  under {max:<5} -> {} item(s), first {:?}",
            found.nodes().map_or(0, <[oxml::NodeId]>::len),
            found.to_str(doc)
        );
        println!(
            "  bound: {:?}",
            ctx.variable(&ExpandedName::local("max"))
                .map(|v| v.to_str(doc))
        );
    }
    match cheap.evaluate_with(doc, &EvalContext::new()) {
        Ok(_) => println!("  unexpectedly evaluated"),
        Err(e) => println!("  with nothing bound        -> {e}"),
    }
    println!(
        "  `evaluate` with variables -> {:?}",
        cheap.evaluate(doc).nodes()
    );
    // A node-set variable takes predicates and steps like any
    // primary expression, and a context node is explicit here too.
    let items = XPath::compile("//item")?.evaluate(doc);
    let first = items
        .nodes()
        .and_then(|n| n.first().copied())
        .expect("items");
    let ctx = EvalContext::new().bind(ExpandedName::local("items"), items);
    let last_price = XPath::compile("string($items[last()]/@price)")?;
    println!(
        "  $items[last()]/@price     -> {}",
        last_price.evaluate_from_with(doc, first, &ctx)?.to_str(doc)
    );
    // Type errors are reported only through the fallible entry point.
    let wrong = XPath::compile("count('not a node-set')")?;
    println!(
        "  count('...') evaluate     -> {}",
        wrong.evaluate(doc).to_str(doc)
    );
    match wrong.evaluate_with(doc, &EvalContext::new()) {
        Ok(_) => println!("  unexpectedly evaluated"),
        Err(e) => println!("  count('...') evaluate_with -> {e}"),
    }
    Ok(())
}

/// `Display` writes an expression back as canonical unabbreviated
/// `XPath` 1.0, which compiles to the same tree: a normal form for a
/// cache key, or a way to show what an abbreviation meant.
fn show_the_rendering() -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== rendering ==");
    for expr in ["//item[@stock > 0][1]/@price", "(//item)[last()]", ".//*"] {
        let compiled = XPath::compile(expr)?;
        let rendered = compiled.to_string();
        assert_eq!(XPath::compile(&rendered)?, compiled, "round trip");
        println!("  {expr:<30} -> {rendered}");
    }
    // The pieces an expression is made of describe themselves.
    let reverse: Vec<&str> = Axis::ALL
        .into_iter()
        .filter(|a| a.is_reverse())
        .map(Axis::name)
        .collect();
    println!("  reverse axes: {reverse:?}");
    println!(
        "  {:?} is {:?}; `nope` is {:?}",
        Axis::from_name("preceding-sibling"),
        Axis::PrecedingSibling.name(),
        Axis::from_name("nope")
    );
    let concat = Func::from_name("concat").expect("defined");
    println!(
        "  {} takes {:?} arguments; {} functions in all",
        concat.name(),
        concat.arity(),
        Func::ALL.len()
    );
    println!("  `{}` spells inequality", BinaryOp::Ne.symbol());
    Ok(())
}

/// An expression is untrusted input too. `compile` applies
/// `Limits::default()`; a front end taking expressions from a network
/// or a model can hand over `Limits::strict()` instead, which refuses
/// deep nesting and long operator runs with an error rather than
/// spending the stack or the CPU on them.
fn show_limits(ns: &oxml::Document) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n== limits ==");
    let strict = Limits::strict();
    let deep = format!("{}1{}", "(".repeat(40), ")".repeat(40));
    match XPath::compile_with_limits(&deep, &strict) {
        Ok(_) => println!("  40 nested parentheses: unexpectedly compiled"),
        Err(e) => println!("  40 nested parentheses under strict -> {e}"),
    }
    let long = format!("{}1", "1+".repeat(1_500));
    match XPath::compile_with_limits(&long, &strict) {
        Ok(_) => println!("  1_500 additions: unexpectedly compiled"),
        Err(e) => println!("  1_500 additions under strict     -> {e}"),
    }
    let bounded = XPath::compile_with_namespaces_and_limits(
        "count(//q:item)",
        &[("q", "urn:u")],
        &strict,
    )?;
    println!(
        "  count(//q:item) under strict     -> {}",
        bounded.evaluate(ns).to_str(ns)
    );
    Ok(())
}
