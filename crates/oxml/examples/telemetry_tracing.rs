// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Telemetry and distributed tracing integration with `oxml`.
//!
//! Demonstrates how `oxml` instruments parsing, `XPath` queries, and
//! serialization under the optional `tracing` feature flag.

use oxml::{XPath, parse};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let xml = r#"
    <library>
        <book id="1" category="fiction">
            <title>1984</title>
            <author>George Orwell</author>
        </book>
        <book id="2" category="science">
            <title>A Brief History of Time</title>
            <author>Stephen Hawking</author>
        </book>
    </library>
    "#;

    println!("Parsing XML payload with tracing spans active...");
    let doc = parse(xml)?;

    println!("Executing compiled XPath query under tracing span...");
    let query = XPath::compile("//book[@category='fiction']/title")?;
    let result = query.evaluate(&doc);

    println!("Found title: {}", result.to_str(&doc));

    println!("Serializing document with tracing span...");
    let serialized = doc.to_xml();
    println!("Document length: {} bytes", serialized.len());

    Ok(())
}
