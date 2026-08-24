// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Run the suite and print a human-readable report.

use std::collections::BTreeMap;

use oxml_conformance::outcome::{Counts, Outcome};
use oxml_conformance::{REAL_TESTS, SUITE_RELEASE, catalog, data_dir, runner};

fn main() -> Result<(), String> {
    let Some(root) = data_dir() else {
        return Err(
            "suite not downloaded; run `cargo run -p oxml-conformance \
             --bin download`"
                .to_owned(),
        );
    };
    let cases = catalog::load(&root)?;
    println!("suite      {SUITE_RELEASE}");
    println!("tests      {} (expected {REAL_TESTS})", cases.len());
    if cases.len() != REAL_TESTS {
        println!(
            "  WARNING: test count drift — the suite is not what we pinned"
        );
    }

    let (results, counts) = runner::run_all(&cases);
    println!("\noverall    {counts}");

    let mut by_sub: BTreeMap<&str, Counts> = BTreeMap::new();
    for r in &results {
        by_sub.entry(&r.submission).or_default().add(r.outcome);
    }
    println!("\nby submission:");
    for (sub, c) in &by_sub {
        println!(
            "  {sub:<10} {:>5.1}% of {:>4} decided   ({} unsupported)",
            c.pass_rate(),
            c.decided(),
            c.unsupported
        );
    }

    let mut reasons: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &results {
        if r.outcome == Outcome::Unsupported {
            *reasons.entry(r.detail.as_str()).or_default() += 1;
        }
    }
    println!("\nunsupported, by reason:");
    for (reason, n) in &reasons {
        println!("  {reason:<28} {n}");
    }

    let failures: Vec<_> = results
        .iter()
        .filter(|r| matches!(r.outcome, Outcome::Fail | Outcome::Panic))
        .collect();
    println!("\nfailures: {}", failures.len());
    for r in failures.iter().take(30) {
        println!("  [{}] {} — {}", r.outcome, r.id, r.detail);
    }
    if failures.len() > 30 {
        println!("  … and {} more", failures.len() - 30);
    }
    Ok(())
}
