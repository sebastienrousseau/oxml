#![no_main]
//! Structure-aware fuzzing of evaluation.
//!
//! Feeding random bytes to `evaluate` would spend nearly all its budget
//! on documents that fail to parse. `Arbitrary` builds a *valid*
//! document and an expression from the same input, so the fuzzer
//! explores the evaluator rather than the parser.

use arbitrary::Arbitrary;
use libfuzzer_sys::fuzz_target;

/// A small, always-well-formed document plus an expression.
#[derive(Debug, Arbitrary)]
struct Case {
    depth: u8,
    breadth: u8,
    with_attrs: bool,
    with_text: bool,
    expr_pick: u8,
}

/// Expressions chosen to cover each axis, node test and function family
/// rather than to be random text — `xpath_compile` already fuzzes the
/// parser.
const EXPRS: &[&str] = &[
    "/", "//a", "//a/b", "//a//b", "//@x", "//a/@*", "//text()",
    "//comment()", "//node()", "//processing-instruction()",
    "//a[1]", "//a[last()]", "//a[position() > 1]", "//a[@x]",
    "//a[@x='1']", "count(//a)", "string(//a)", "//a | //b",
    "//a/parent::*", "//a/ancestor::*", "//a/ancestor-or-self::*",
    "//a/following-sibling::*", "//a/preceding-sibling::*",
    "//a/descendant::*", "//a/self::a", "normalize-space(//a)",
    "substring(string(//a), 2, 3)", "//a[count(b) > 0]",
    "local-name(//a)", "namespace-uri(//a)", "sum(//a)",
    "//a[string-length(.) > 2]", "round(1.5)", "floor(-1.5)",
];

fn build(c: &Case) -> String {
    let depth = usize::from(c.depth % 6) + 1;
    let breadth = usize::from(c.breadth % 4) + 1;
    let mut s = String::from("<r>");
    for _ in 0..depth {
        for _ in 0..breadth {
            s.push_str(if c.with_attrs { "<a x=\"1\">" } else { "<a>" });
            if c.with_text {
                s.push_str("txt");
            }
            s.push_str("<b/>");
            s.push_str("</a>");
        }
    }
    s.push_str("</r>");
    s
}

fuzz_target!(|case: Case| {
    let src = build(&case);
    let Ok(doc) = oxml::parse(&src) else {
        // `build` only emits well-formed documents; if that stops being
        // true the generator is wrong, not the parser.
        panic!("generator produced a malformed document: {src}");
    };
    let expr = EXPRS[usize::from(case.expr_pick) % EXPRS.len()];
    let Ok(x) = oxml::XPath::compile(expr) else {
        panic!("fixed expression failed to compile: {expr}");
    };

    // Evaluation must be total: every expression against every document
    // yields a value, never a panic.
    let value = x.evaluate(&doc);
    let _ = value.to_str(&doc);
    if let Some(nodes) = value.nodes() {
        for n in nodes {
            // Every returned node must be a real node of *this*
            // document — a stale or out-of-range NodeId would panic in
            // the accessors.
            let _ = doc.kind(*n);
            let _ = doc.text(*n);
        }
    }
});
