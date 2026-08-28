// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! How many allocations a parse costs, per node.
//!
//! The README publishes this number. A published figure with nothing
//! measuring it drifts the moment the parser changes, and nobody
//! notices because prose does not fail a build. This counts the
//! allocations a real parse performs and holds the result to a ceiling.
//!
//! The ceiling is deliberately a ceiling and not an equality: an
//! allocator-level count varies with capacity growth, so pinning it
//! exactly would make the test fail for reasons that are not
//! regressions. Going *under* it is the good direction and never fails.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::Mutex;
use std::sync::atomic::{AtomicIsize, AtomicUsize, Ordering};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);

// Counting is per-thread. A global flag also caught allocations and
// frees from whatever else the harness was running, and a foreign
// `dealloc` of memory allocated before counting began drove the live
// total negative -- which reported a tree as holding zero bytes.
// Measuring on its own thread means every free seen is a free of
// something this measurement allocated.
//
// `Cell<bool>` has no destructor, so `const` initialisation makes this
// access allocation-free and safe to reach from inside the allocator.
thread_local! {
    static COUNTING: core::cell::Cell<bool> =
        const { core::cell::Cell::new(false) };
}

/// Whether the calling thread is being measured.
fn counting() -> bool {
    COUNTING.try_with(core::cell::Cell::get).unwrap_or(false)
}
/// Bytes currently held, and the high-water mark of that figure.
///
/// Counting allocations answers "how often"; this answers "how much at
/// once", which is the question a streaming reader exists to change.
static LIVE: AtomicIsize = AtomicIsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// Records a change in live bytes and keeps the high-water mark.
fn note(delta: isize) {
    if !counting() {
        return;
    }
    let live = LIVE.fetch_add(delta, Ordering::Relaxed) + delta;
    if live > 0 {
        let live = live as usize;
        let _ = PEAK.fetch_max(live, Ordering::Relaxed);
    }
}

struct Counter;

// SAFETY-equivalent note: this delegates every operation to `System`
// and only adds a counter, so it inherits `System`'s guarantees. The
// crate forbids `unsafe`, but a test may use it, and an allocator
// cannot be written without it.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if counting() {
            let _ = ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        note(layout.size() as isize);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        note(-(layout.size() as isize));
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        if counting() {
            let _ = ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        note(new_size as isize - layout.size() as isize);
        unsafe { System.realloc(ptr, layout, new_size) }
    }
}

#[global_allocator]
static ALLOCATOR: Counter = Counter;

/// Serialises measurements.
///
/// The counter is global, so two measurements running at once report
/// each other's allocations. Without this both tests in this file
/// reported the same figure and the smaller one looked like a
/// regression.
static MEASURING: Mutex<()> = Mutex::new(());

/// Runs `f` on a thread that counts its own allocations.
///
/// The value is dropped on that thread before counting stops, so what
/// `f` returns does not escape into the next measurement.
fn on_a_measured_thread<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        scope
            .spawn(|| {
                COUNTING.with(|c| c.set(true));
                let value = f();
                COUNTING.with(|c| c.set(false));
                value
            })
            .join()
            .expect("the measured thread must not panic")
    })
}

/// Allocations performed while `f` runs.
fn measure<T: Send>(f: impl FnOnce() -> T + Send) -> (T, usize) {
    let _guard = MEASURING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ALLOCATIONS.store(0, Ordering::SeqCst);
    let value = on_a_measured_thread(f);
    (value, ALLOCATIONS.load(Ordering::SeqCst))
}

/// The high-water mark of bytes held while `f` runs.
///
/// The source document is allocated by the caller before measuring, so
/// it is not counted: what this reports is what the parser or reader
/// holds *on top of* the input it was handed.
fn measure_peak<T: Send>(f: impl FnOnce() -> T + Send) -> (T, usize) {
    let _guard = MEASURING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    LIVE.store(0, Ordering::SeqCst);
    PEAK.store(0, Ordering::SeqCst);
    let value = on_a_measured_thread(f);
    (value, PEAK.load(Ordering::SeqCst))
}

/// A document with a realistic mix of elements, attributes and text.
fn corpus(elements: usize) -> String {
    use std::fmt::Write as _;
    let mut out = String::from("<catalogue xmlns:m=\"urn:example\">");
    for i in 0..elements {
        let _ = write!(
            out,
            "<item id=\"i{i}\" m:sku=\"S{i}\"><name>Item {i}</name>\
             <price currency=\"GBP\">{i}.99</price></item>"
        );
    }
    out.push_str("</catalogue>");
    out
}

#[test]
fn a_parse_costs_a_bounded_number_of_allocations_per_node() {
    // The README publishes 0.50. The ceiling sits just above the
    // measured figure: close enough that a regression trips it, loose
    // enough that allocator capacity growth does not. It has come down
    // from 4.5 as child and attribute lists were flattened, names
    // interned by borrowed parts, a dead resolve removed, and names
    // made to borrow the input rather than be copied out of it.
    //
    // The step from 1.13 to 0.50 is the document owning its input:
    // text nodes, comments and attribute values are ranges into it
    // rather than strings of their own, so a document that expands no
    // entities allocates nothing for its character data at all.
    const CEILING: f64 = 0.6;

    let source = corpus(2_000);
    let (doc, allocations) =
        measure(|| oxml::parse(&source).expect("well-formed"));
    let nodes = doc.len();
    let per_node = allocations as f64 / nodes as f64;

    println!(
        "{allocations} allocations for {nodes} nodes = {per_node:.2} per node"
    );
    assert!(nodes > 10_000, "corpus too small to be meaningful: {nodes}");
    assert!(
        per_node <= CEILING,
        "{per_node:.2} allocations per node exceeds the ceiling of \
         {CEILING:.1}; the README publishes 1.13"
    );
}

#[test]
fn parsing_does_not_allocate_per_byte_of_text() {
    // A long text node must not cost an allocation for each character
    // or each chunk. This is the shape that turns a large document
    // into an allocator benchmark.
    let long = "x".repeat(1_000_000);
    let source = format!("<a>{long}</a>");
    let (doc, allocations) =
        measure(|| oxml::parse(&source).expect("well-formed"));

    println!("{allocations} allocations for a 1 MB text node");
    // Root, element, text -- and one namespace node. `xml` is bound by
    // specification for every element, so the root element carries a
    // node for it and every descendant inherits it through the
    // `namespace::` ancestor walk. That is one node per document, not
    // one per element.
    assert_eq!(
        doc.len(),
        4,
        "root, element, text, the implicit xml binding"
    );
    assert!(
        allocations < 100,
        "{allocations} allocations for one text node suggests per-chunk growth"
    );
}

/// What streaming actually saves, measured rather than asserted.
///
/// It does **not** let a caller read a document larger than memory:
/// [`oxml::stream::Reader`] is handed a `&str`, and normalising line
/// endings copies it once more. What it removes is the tree — the
/// arena, the interned names, and every node that outlives the event
/// that produced it. This holds that saving to a ratio so that a
/// change which quietly reintroduces retention fails the build.
#[test]
fn reading_events_holds_less_than_building_a_tree() {
    use oxml::stream::{Event, Reader};

    let source = corpus(2_000);

    let (tree, tree_peak) =
        measure_peak(|| oxml::parse(&source).expect("well-formed"));
    let nodes = tree.len();
    drop(tree);

    let (events, stream_peak) = measure_peak(|| {
        let mut reader = Reader::new(&source).expect("well-formed");
        let mut seen = 0usize;
        while let Some(event) = reader.next_event().expect("well-formed") {
            // Counted and dropped: nothing accumulates, which is the
            // whole point of the entry point.
            if matches!(event, Event::StartElement { .. }) {
                seen += 1;
            }
        }
        seen
    });

    println!(
        "{nodes} nodes: tree holds {tree_peak} bytes at peak, \
         reading holds {stream_peak} ({:.0}% less)",
        100.0 - (stream_peak as f64 / tree_peak as f64) * 100.0
    );
    // `item`, `name` and `price` for each, and the `catalogue`
    // wrapping them.
    assert_eq!(events, 2_000 * 3 + 1, "every element was seen");

    // The reader still holds a normalised copy of the input, so the
    // floor is the document size, not zero.
    assert!(
        stream_peak >= source.len(),
        "a normalised copy of the input is held: {stream_peak} bytes \
         for a {} byte document",
        source.len()
    );
    assert!(
        stream_peak * 2 < tree_peak,
        "reading events must hold less than half what a tree holds, \
         but held {stream_peak} against {tree_peak}"
    );
}

/// What reading events costs in allocations, against parsing.
///
/// Peak memory is not the only figure that matters. `oxml-wasm`'s
/// benchmark found reading a document as events takes roughly twice
/// as long as parsing it into a tree, which is the opposite of what
/// "streaming" suggests, and this is where to look for why.
#[test]
fn reading_events_allocation_cost() {
    use oxml::stream::Reader;

    let source = corpus(2_000);

    let (_, parse_allocs) =
        measure(|| oxml::parse(&source).expect("well-formed"));

    let (events, stream_allocs) = measure(|| {
        let mut reader = Reader::new(&source).expect("well-formed");
        let mut n = 0usize;
        while reader.next_event().expect("well-formed").is_some() {
            n += 1;
        }
        n
    });

    println!(
        "parse: {parse_allocs} allocations; stream: {stream_allocs} for \
         {events} events = {:.2} per event",
        stream_allocs as f64 / events as f64
    );
}

/// Reading from a byte source holds a bounded amount, whatever the
/// document's size.
///
/// This is the difference between `Reader::new` and
/// `Reader::from_reader`. Both build no tree, but `new` is handed the
/// whole document and keeps it; `from_reader` keeps the construct it
/// is reading and drops what it has passed. A document larger than
/// memory is readable only by the second.
#[test]
fn reading_from_a_source_does_not_hold_the_document() {
    use oxml::stream::{Event, Reader};
    use std::io::Cursor;

    let small = corpus(2_000);
    let large = corpus(20_000);
    assert!(
        large.len() > small.len() * 5,
        "the two sizes must differ enough to tell a bound from a slope"
    );

    let peak_of = |source: &str| {
        let owned = source.as_bytes().to_vec();
        let (events, peak) = measure_peak(move || {
            let mut reader =
                Reader::from_reader(Cursor::new(owned)).expect("well-formed");
            let mut n = 0usize;
            while let Some(event) = reader.next_event().expect("valid") {
                if matches!(event, Event::StartElement { .. }) {
                    n += 1;
                }
            }
            n
        });
        assert!(events > 0);
        peak
    };

    let small_peak = peak_of(&small);
    let large_peak = peak_of(&large);

    println!(
        "{} KB held {small_peak} bytes; {} KB held {large_peak}",
        small.len() / 1024,
        large.len() / 1024
    );

    // Ten times the document, nothing like ten times the memory. The
    // bound is the buffer and the open-element stack, not the input.
    assert!(
        large_peak < small_peak * 2,
        "holding {large_peak} for a document ten times the size of one \
         that held {small_peak} is not a bound, it is a slope"
    );
    assert!(
        large_peak < large.len(),
        "held {large_peak} bytes of a {} byte document; the point of \
         reading from a source is not to keep it",
        large.len()
    );
}
