// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Bounding what a document can cost you.
//!
//! Every limit exists because some document shape turns a small input
//! into a large amount of work. The defaults are chosen to accept real
//! documents and reject attacks; `strict` and `permissive` move the
//! line, and the individual fields move it precisely.
//!
//! Run with:
//!
//! ```text
//! cargo run --example apply_limits
//! ```

use oxml::{Edition, ErrorKind, Limits, MAX_DEPTH, parse, parse_with};

/// The billion laughs attack: nine entities, each ten copies of the
/// last. Under 1 KB of input, 10^9 characters of output.
///
/// The default budget is 10 MB, so a document has to genuinely aim
/// past it to be refused -- five levels expand to 400 KB and are
/// accepted, because they are not an attack.
const BILLION_LAUGHS: &str = r#"<?xml version="1.0"?>
<!DOCTYPE lolz [
  <!ENTITY l0 "haha">
  <!ENTITY l1 "&l0;&l0;&l0;&l0;&l0;&l0;&l0;&l0;&l0;&l0;">
  <!ENTITY l2 "&l1;&l1;&l1;&l1;&l1;&l1;&l1;&l1;&l1;&l1;">
  <!ENTITY l3 "&l2;&l2;&l2;&l2;&l2;&l2;&l2;&l2;&l2;&l2;">
  <!ENTITY l4 "&l3;&l3;&l3;&l3;&l3;&l3;&l3;&l3;&l3;&l3;">
  <!ENTITY l5 "&l4;&l4;&l4;&l4;&l4;&l4;&l4;&l4;&l4;&l4;">
  <!ENTITY l6 "&l5;&l5;&l5;&l5;&l5;&l5;&l5;&l5;&l5;&l5;">
  <!ENTITY l7 "&l6;&l6;&l6;&l6;&l6;&l6;&l6;&l6;&l6;&l6;">
  <!ENTITY l8 "&l7;&l7;&l7;&l7;&l7;&l7;&l7;&l7;&l7;&l7;">
]>
<lolz>&l8;</lolz>
"#;

fn main() {
    println!("default depth limit: {MAX_DEPTH}");

    // Entity expansion is bounded per *document*, not per reference,
    // so a thousand small expansions cost the same budget as one large
    // one. Bounding each reference separately still lets a quadratic
    // blowup through.
    let started = std::time::Instant::now();
    match parse(BILLION_LAUGHS) {
        Ok(_) => println!("\nbillion laughs: expanded (this would be a bug)"),
        Err(e) => println!("\nbillion laughs: {e}"),
    }
    // The budget stops the expansion rather than letting it finish and
    // measuring afterwards -- but it still permits 10 MB of work from
    // under 1 KB of input, which is around 50 ms of a release-build
    // core. That is a deliberate trade: it is generous enough that no
    // real document is refused. A service taking untrusted XML under
    // load should say so explicitly and use `strict`, which caps
    // expansion at 100 KB and rejects the same document in well under
    // a millisecond.
    println!("                refused in {:?}", started.elapsed());
    let started = std::time::Instant::now();
    let strict = parse_with(BILLION_LAUGHS, Limits::strict());
    println!(
        "                under strict: {} in {:?}",
        if strict.is_err() {
            "refused"
        } else {
            "accepted"
        },
        started.elapsed()
    );

    // External entities are never fetched. That is not a limit you can
    // raise -- there is no code to fetch them, so XXE is structurally
    // impossible rather than switched off by default.
    let xxe =
        r#"<!DOCTYPE d [<!ENTITY x SYSTEM "file:///etc/passwd">]><d>&x;</d>"#;
    match parse(xxe) {
        Ok(doc) => println!(
            "XXE attempt:    parsed, &x; is {:?}",
            doc.text(doc.root())
        ),
        Err(e) => println!("XXE attempt:    {e}"),
    }

    println!("\n== depth ==");
    // Recursion is bounded so that a deeply nested document returns an
    // error instead of overflowing the stack, which no caller can
    // catch.
    let deep = format!("{}{}", "<a>".repeat(400), "</a>".repeat(400));
    match parse(&deep) {
        Ok(_) => println!("  400 levels: accepted"),
        Err(e) if e.kind == ErrorKind::DepthLimitExceeded => {
            println!("  400 levels: refused, {e}");
        }
        Err(e) => println!("  400 levels: {e}"),
    }

    println!("\n== choosing a profile ==");
    let shallow = format!("{}{}", "<a>".repeat(20), "</a>".repeat(20));
    for (name, limits) in [
        ("default   ", Limits::default()),
        ("strict    ", Limits::strict()),
        ("permissive", Limits::permissive()),
    ] {
        let ok = parse_with(&shallow, limits).is_ok();
        println!(
            "  {name}: max_depth={:<5} 20 levels ok? {ok}",
            limits.max_depth
        );
    }

    println!("\n== one field at a time ==");
    // `Limits` is `#[non_exhaustive]`, so it cannot be built with
    // struct-literal syntax: a bound added in a later version would
    // otherwise break every caller that wrote one out in full. Start
    // from a profile and adjust the fields that matter -- they are all
    // public, so a caller who knows their documents can tighten
    // exactly one bound and inherit the rest.
    let mut tight = Limits::default();
    tight.max_depth = 3;
    let five = format!("{}{}", "<a>".repeat(5), "</a>".repeat(5));
    match parse_with(&five, tight) {
        Ok(_) => println!("  5 levels under max_depth=3: accepted (a bug)"),
        Err(e) => println!("  5 levels under max_depth=3: {e}"),
    }

    println!("\n== which edition of XML 1.0 ==");
    // The 4th and 5th editions disagree about which characters may
    // start a name, and the disagreement is not a widening: each
    // allows names the other rejects. Pick the one your documents were
    // authored against.
    let doc = "<\u{2118}/>"; // U+2118, a name character in the 5th only
    for (name, edition) in
        [("fourth", Edition::Fourth), ("fifth", Edition::Fifth)]
    {
        let mut limits = Limits::default();
        limits.edition = edition;
        println!("  {name:<7}: {:?}", parse_with(doc, limits).is_ok());
    }
}
