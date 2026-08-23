#![no_main]
//! Arbitrary bytes must never panic the parser.
//!
//! The contract is total: any input at all produces `Ok` or `Err`, and
//! nothing else. A panic is a denial of service for every caller that
//! parses input it did not write, which is all four of this crate's
//! front ends.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Only valid UTF-8 reaches `parse`, which takes `&str`. Feeding raw
    // bytes and discarding invalid ones would waste most of the corpus,
    // so recover the longest valid prefix instead.
    let text = match core::str::from_utf8(data) {
        Ok(s) => s,
        Err(e) => match core::str::from_utf8(&data[..e.valid_up_to()]) {
            Ok(s) => s,
            Err(_) => return,
        },
    };

    if let Ok(doc) = oxml::parse(text) {
        // A successful parse must produce a walkable tree. Touching
        // every accessor here means a malformed-but-accepted document
        // cannot hide a broken invariant behind a lazy field.
        for id in doc.descendants() {
            let _ = doc.kind(id);
            let _ = doc.parent(id);
            let _ = doc.children(id);
            let _ = doc.is_element(id);
            let _ = doc.element_name(id);
            let _ = doc.attributes(id);
            let _ = doc.attribute_nodes(id);
            let _ = doc.attribute(id, "x");
            let _ = doc.text(id);
        }
        let _ = doc.root_element();
        let _ = doc.len();
        let _ = doc.is_empty();
    }
});
