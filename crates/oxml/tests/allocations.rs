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
use std::sync::atomic::{AtomicUsize, Ordering};

static ALLOCATIONS: AtomicUsize = AtomicUsize::new(0);
static COUNTING: AtomicUsize = AtomicUsize::new(0);

struct Counter;

// SAFETY-equivalent note: this delegates every operation to `System`
// and only adds a counter, so it inherits `System`'s guarantees. The
// crate forbids `unsafe`, but a test may use it, and an allocator
// cannot be written without it.
unsafe impl GlobalAlloc for Counter {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) == 1 {
            let _ = ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) };
    }

    unsafe fn realloc(
        &self,
        ptr: *mut u8,
        layout: Layout,
        new_size: usize,
    ) -> *mut u8 {
        if COUNTING.load(Ordering::Relaxed) == 1 {
            let _ = ALLOCATIONS.fetch_add(1, Ordering::Relaxed);
        }
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

/// Allocations performed while `f` runs.
fn measure<T>(f: impl FnOnce() -> T) -> (T, usize) {
    let _guard = MEASURING
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    ALLOCATIONS.store(0, Ordering::SeqCst);
    COUNTING.store(1, Ordering::SeqCst);
    let value = f();
    COUNTING.store(0, Ordering::SeqCst);
    (value, ALLOCATIONS.load(Ordering::SeqCst))
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
    // The README publishes 4.1. The ceiling sits just above the
    // measured figure: close enough that a regression trips it, loose
    // enough that allocator capacity growth does not.
    const CEILING: f64 = 4.5;

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
         {CEILING:.1}; the README publishes 4.1"
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
    assert_eq!(doc.len(), 3, "root, element, text");
    assert!(
        allocations < 100,
        "{allocations} allocations for one text node suggests per-chunk growth"
    );
}
