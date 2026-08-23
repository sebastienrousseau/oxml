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
    Ok(())
}
