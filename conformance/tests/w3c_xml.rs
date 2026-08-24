// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The W3C XML Conformance Test Suite, ratcheted against a baseline.
//!
//! Run `cargo run -p oxml-conformance --bin download` first. Without
//! the suite these tests skip rather than fail, so a normal
//! `cargo test` does not need a network.
//!
//! Regenerate the baseline deliberately, never automatically:
//!
//! ```text
//! OXML_UPDATE_BASELINE=1 cargo test -p oxml-conformance
//! ```

use std::path::Path;

use oxml_conformance::{REAL_TESTS, baseline, catalog, require_suite, runner};

fn baseline_path() -> std::path::PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("baselines")
        .join("w3c-xml.tsv")
}

#[test]
fn the_suite_matches_its_baseline() {
    let root = require_suite!();
    let cases = catalog::load(&root).expect("catalog loads");

    // Test-count drift means the suite on disk is not the release we
    // pinned — every rate computed from it would be against a
    // different denominator. This caught a loader bug that silently
    // dropped all 159 Sun tests.
    assert_eq!(
        cases.len(),
        REAL_TESTS,
        "test count drift: expected {REAL_TESTS}, found {}",
        cases.len()
    );

    let (results, counts) = runner::run_all(&cases);
    let rendered = baseline::render(&results, &counts);

    let path = baseline_path();
    if std::env::var_os("OXML_UPDATE_BASELINE").is_some() {
        std::fs::create_dir_all(path.parent().expect("has a parent"))
            .expect("create baselines dir");
        std::fs::write(&path, &rendered).expect("write baseline");
        eprintln!("baseline updated: {counts}");
        return;
    }

    let previous = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "no baseline at {}; generate one with \
             OXML_UPDATE_BASELINE=1 cargo test -p oxml-conformance",
            path.display()
        )
    });

    let differences = baseline::diff(
        &baseline::parse(&previous),
        &baseline::parse(&rendered),
    );
    assert!(
        differences.is_empty(),
        "conformance changed against the baseline.\n\n{}\n\n\
         If this is an intended improvement, regenerate with:\n  \
         OXML_UPDATE_BASELINE=1 cargo test -p oxml-conformance\n\n\
         Note that an *improvement* fails here too, on purpose: a pass \
         rate that drifts upward because tests started being skipped \
         rather than passing is indistinguishable from real progress \
         unless every change is reviewed.",
        differences
            .iter()
            .take(40)
            .map(String::as_str)
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn no_input_in_the_suite_panics_the_parser() {
    // Separate from the ratchet: a panic is never acceptable, at any
    // baseline. A caller cannot catch it.
    let root = require_suite!();
    let cases = catalog::load(&root).expect("catalog loads");
    let (results, _) = runner::run_all(&cases);
    let panics: Vec<&str> = results
        .iter()
        .filter(|r| r.outcome == oxml_conformance::outcome::Outcome::Panic)
        .map(|r| r.id.as_str())
        .collect();
    assert!(panics.is_empty(), "these inputs panicked: {panics:?}");
}
