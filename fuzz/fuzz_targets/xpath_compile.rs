#![no_main]
//! Arbitrary expressions must never panic the XPath compiler, and
//! whatever compiles must evaluate and drop without exhausting the
//! stack.
//!
//! An expression is untrusted input in every front end: the CLI takes
//! one from a shell, the MCP server from a model, the WASM bindings
//! from JavaScript. Compiling is only half of it: a tree the parser
//! accepted used to be walked recursively by `evaluate`, `Clone` and
//! `Drop`, so a flat `1+1+1+...` compiled and then overflowed the
//! stack one step later. The whole life of the expression runs here,
//! on the fuzzer's own thread, so a recursion that scales with the
//! input's length aborts the run rather than hiding.

use libfuzzer_sys::fuzz_target;

/// Small, but with a few nodes of each kind so paths and predicates
/// have something to select.
const DOC: &str = "<r a=\"1\"><x>t<y/></x><!--c--><?p d?><x/></r>";

fuzz_target!(|data: &[u8]| {
    let Ok(expr) = core::str::from_utf8(data) else {
        return;
    };
    let Ok(x) = oxml::XPath::compile(expr) else {
        return;
    };
    // Compiling must be deterministic: the same text twice gives the
    // same tree. A parser carrying state between calls would show up
    // here.
    let again = oxml::XPath::compile(expr).expect("compiled once already");
    assert_eq!(x, again, "compilation is not deterministic");
    // And `Display` must write what the parser reads: the canonical
    // rendering compiles back to the same tree, and renders the same
    // again. A rendering the parser rejects, or reads differently, is
    // a bug in one of the two.
    let rendered = x.to_string();
    let reparsed = oxml::XPath::compile(&rendered)
        .unwrap_or_else(|e| panic!("`{expr}` rendered as `{rendered}`: {e}"));
    assert_eq!(reparsed, x, "`{expr}` rendered as `{rendered}`");
    assert_eq!(
        reparsed.to_string(),
        rendered,
        "rendering is not a fixed point"
    );

    let doc = oxml::parse(DOC).expect("the fixture is well-formed");
    let value = x.evaluate(&doc);
    let _ = value.to_str(&doc);
    let _ = value.to_number(&doc);
    drop(value);
    drop(again);
    drop(x);
});
