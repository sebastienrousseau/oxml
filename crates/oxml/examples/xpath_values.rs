// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The four `XPath` value types, and evaluating from a context node.
//!
//! Run with:
//!
//! ```text
//! cargo run --example xpath_values
//! ```

use oxml::{XPath, parse, xpath::Value};

const DOC: &str = r#"<?xml version="1.0"?>
<shop>
  <item price="9.99" stock="3">Tea</item>
  <item price="4.50" stock="0">Coffee</item>
  <item price="12.00" stock="7">Cocoa</item>
</shop>
"#;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let doc = parse(DOC)?;

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
