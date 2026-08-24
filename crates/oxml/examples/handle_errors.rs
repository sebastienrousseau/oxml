// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reporting a parse failure to a human.
//!
//! Errors carry a byte offset rather than a formatted message, so a
//! caller can point at the problem in whatever way suits it -- a line
//! and column, a caret under the source, or a span in an editor.
//!
//! Run with:
//!
//! ```text
//! cargo run --example handle_errors
//! ```

use oxml::{ErrorKind, parse};

const BROKEN: &[(&str, &str)] = &[
    ("<a>text", "a tag left open"),
    ("<a></b>", "mismatched tags"),
    ("<a>&nope;</a>", "an entity nobody declared"),
    ("<a x='1' x='2'/>", "the same attribute twice"),
    ("<p:a/>", "a prefix with no declaration"),
    ("<a/><b/>", "two root elements"),
    ("", "nothing at all"),
    ("<a>]]></a>", "a literal CDATA terminator"),
    ("<!-- no -- good -->", "a double hyphen in a comment"),
];

fn main() {
    for (input, why) in BROKEN {
        let Err(error) = parse(input) else {
            println!("{input:?} unexpectedly parsed");
            continue;
        };

        // The offset is a byte index into the input, and `line_column`
        // turns it into the position a person would count -- one-based,
        // and in characters rather than bytes so a line of Japanese
        // reports the column an editor shows.
        let (line, column) = error.line_column(input);
        println!("{why}:");
        println!("  {input:?}");
        println!("  {error}");
        println!("  line {line}, column {column}");

        // Matching on the kind is how a caller reacts differently to
        // different failures -- retrying, or suggesting a fix.
        let advice = match &error.kind {
            ErrorKind::UnexpectedEof => "the document is truncated",
            ErrorKind::MismatchedEndTag { expected, found } => {
                &format!("close <{expected}>, not <{found}>")
            }
            ErrorKind::UnknownEntity(name) => {
                &format!("declare &{name}; or write &amp;{name};")
            }
            ErrorKind::DuplicateAttribute(name) => {
                &format!("remove one of the two {name} attributes")
            }
            ErrorKind::UnboundPrefix(p) => &format!("add an xmlns:{p}"),
            ErrorKind::TrailingContent => "wrap both elements in a root",
            ErrorKind::NoRootElement => "an XML document needs one element",
            ErrorKind::IllegalCdataEnd => "write ]]&gt; instead",
            ErrorKind::MalformedComment => "-- may not appear in a comment",
            _ => "see the message above",
        };
        println!("  advice: {advice}\n");
    }

    // Showing the caret is the reason the offset is exposed at all.
    let input =
        "<config>\n  <name>ok</name>\n  <port>8080</hostname>\n</config>";
    if let Err(error) = parse(input) {
        let (line, column) = error.line_column(input);
        let source = input.lines().nth(line - 1).unwrap_or_default();
        println!("== pointing at the problem ==");
        println!("{line:>3} | {source}");
        println!("    | {}^ {}", " ".repeat(column - 1), error.kind);
    }
}
