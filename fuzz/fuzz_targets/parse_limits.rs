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
    let bound = usize::from(data.first().copied().unwrap_or(0));
    let mut limits = Limits::strict();
    limits.max_input_bytes = Some(bound);
    limits.max_document_bytes = Some(bound);
    limits.max_nodes = Some(bound);
    limits.max_dtd_depth = bound % 4;
    let bytes = oxml::parse_bytes_with(data, limits);
    if data.len() > bound {
        assert_eq!(bytes.unwrap_err().kind, oxml::ErrorKind::InputTooLarge);
    } else if let Ok(doc) = bytes {
        assert!(doc.len() <= bound);
        assert!(
            oxml::parse_bytes(data).is_ok(),
            "tighter limits accepted more"
        );
    }
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
