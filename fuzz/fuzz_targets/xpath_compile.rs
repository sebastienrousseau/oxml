#![no_main]
//! Arbitrary expressions must never panic the XPath compiler.
//!
//! An expression is untrusted input in every front end: the CLI takes
//! one from a shell, the MCP server from a model, the WASM bindings
//! from JavaScript.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(expr) = core::str::from_utf8(data) else {
        return;
    };
    if let Ok(x) = oxml::XPath::compile(expr) {
        // Compiling must be deterministic: the same text twice gives
        // the same tree. A parser carrying state between calls would
        // show up here.
        let again = oxml::XPath::compile(expr).expect("compiled once already");
        assert_eq!(
            format!("{:?}", x.expr()),
            format!("{:?}", again.expr()),
            "compilation is not deterministic"
        );
    }
});
