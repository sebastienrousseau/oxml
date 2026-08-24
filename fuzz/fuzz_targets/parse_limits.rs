#![no_main]
//! Limits must hold for arbitrary input, and must not change whether a
//! document is *accepted* except by rejecting it.
//!
//! Two properties:
//!   - parsing under `strict()` never panics;
//!   - anything `strict()` accepts, `default()` also accepts.
//! The second is the one that catches a limit implemented as a
//! side-effect rather than a bound — a check that mutates parser state
//! would let a tighter limit accept something a looser one rejects.

use libfuzzer_sys::fuzz_target;
use oxml::Limits;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = core::str::from_utf8(data) else {
        return;
    };

    let strict = oxml::parse_with(text, Limits::strict());
    let default = oxml::parse_with(text, Limits::default());

    if strict.is_ok() {
        assert!(
            default.is_ok(),
            "strict() accepted a document default() rejected"
        );
    }
});
