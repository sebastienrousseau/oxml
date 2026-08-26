#![no_main]
//! The event reader must agree with the tree parser on arbitrary input.
//!
//! Two things are checked, and the second is the one that matters.
//!
//! First, totality: any input at all produces `Ok` or `Err` and never a
//! panic — the same contract [`oxml::parse`] carries, for the same
//! reason. A hang counts as a failure here too, which is the property
//! that was missing when a `CDATA` section consumed nothing and the
//! reader looped forever.
//!
//! Second, agreement. `Reader` and `parse` run the same scanner, so a
//! document one accepts the other must accept, and the elements and
//! text they find must match. That is a much stronger statement than
//! "does not panic": a reader that rejected every input would satisfy
//! totality and fail this immediately.

use libfuzzer_sys::fuzz_target;
use oxml::stream::{Event, Reader};

/// Every element name and all the text, read as events.
fn by_event(text: &str) -> Result<(Vec<String>, String), oxml::Error> {
    let mut reader = Reader::new(text)?;
    let mut names = Vec::new();
    let mut content = String::new();
    while let Some(event) = reader.next_event()? {
        match event {
            Event::StartElement { name, .. } => names.push(name.local),
            Event::Text(t) => content.push_str(&t),
            _ => {}
        }
    }
    Ok((names, content))
}

fuzz_target!(|data: &[u8]| {
    // As in `parse`: only valid UTF-8 reaches the reader, so recover
    // the longest valid prefix rather than discarding the input.
    let text = match core::str::from_utf8(data) {
        Ok(s) => s,
        Err(e) => match core::str::from_utf8(&data[..e.valid_up_to()]) {
            Ok(s) => s,
            Err(_) => return,
        },
    };

    let streamed = by_event(text);
    let parsed = oxml::parse(text);

    match (&streamed, &parsed) {
        (Ok((names, content)), Ok(doc)) => {
            let from_tree: Vec<String> = doc
                .descendants()
                .filter_map(|id| doc.element_name(id).map(|n| n.local.clone()))
                .collect();
            assert_eq!(
                *names, from_tree,
                "element sequences differ for {text:?}"
            );
            assert_eq!(
                *content,
                doc.text(doc.root()),
                "text differs for {text:?}"
            );
        }
        (Err(s), Err(p)) => {
            // The same refusal, not merely both refusing.
            assert_eq!(s.kind, p.kind, "error kinds differ for {text:?}");
            assert_eq!(
                s.offset, p.offset,
                "error offsets differ for {text:?}"
            );
        }
        (Ok(_), Err(p)) => {
            panic!("the reader accepted what `parse` refused ({p}): {text:?}")
        }
        (Err(s), Ok(_)) => {
            panic!("the reader refused what `parse` accepted ({s}): {text:?}")
        }
    }
});
