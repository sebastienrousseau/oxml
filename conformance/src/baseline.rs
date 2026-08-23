// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! The baseline ratchet.
//!
//! Results are compared against a committed file rather than asserted
//! against a target. A regression fails, and so does an **improvement**
//! — which sounds perverse until you have watched a pass rate drift
//! upward because a test started being skipped rather than passing.
//! Requiring the baseline to be regenerated deliberately means every
//! change to the number is a reviewed change.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use crate::outcome::{Counts, Outcome};
use crate::runner::Scored;

/// Render results as the baseline format.
///
/// One line per non-passing test, sorted, with a header carrying the
/// counts. Passing tests are omitted: there are ~2,000 of them and
/// listing them would bury the signal.
///
/// `detail` is written for a human but is **not** compared, because
/// error wording changes far more often than behaviour.
#[must_use]
pub fn render(results: &[Scored], counts: &Counts) -> String {
    let mut out = String::new();
    let _ = writeln!(
        out,
        "#counts\tpass={}\tfail={}\tpanic={}\tunsupported={}\tblocked={}\ttotal={}",
        counts.pass,
        counts.fail,
        counts.panic,
        counts.unsupported,
        counts.blocked,
        counts.total(),
    );
    let mut rows: Vec<&Scored> = results
        .iter()
        .filter(|r| r.outcome != Outcome::Pass)
        .collect();
    rows.sort_by(|a, b| a.id.cmp(&b.id));
    for r in rows {
        let _ = writeln!(
            out,
            "{}\t{}\t{}\t{}",
            r.submission, r.id, r.outcome, r.detail
        );
    }
    out
}

/// The comparable part of a baseline: counts, and outcome by test id.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Parsed {
    /// The `#counts` header.
    pub counts: BTreeMap<String, usize>,
    /// Outcome per test id, excluding passes.
    pub outcomes: BTreeMap<String, String>,
}

/// Read a baseline file.
#[must_use]
pub fn parse(text: &str) -> Parsed {
    let mut p = Parsed::default();
    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("#counts\t") {
            for field in rest.split('\t') {
                if let Some((k, Ok(n))) =
                    field.split_once('=').map(|(k, v)| (k, v.parse()))
                {
                    let _ = p.counts.insert(k.to_owned(), n);
                }
            }
            continue;
        }
        if line.starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let mut f = line.split('\t');
        let (Some(_sub), Some(id), Some(outcome)) =
            (f.next(), f.next(), f.next())
        else {
            continue;
        };
        let _ = p.outcomes.insert(id.to_owned(), outcome.to_owned());
    }
    p
}

/// Compare a fresh run against a baseline, returning every difference.
#[must_use]
pub fn diff(baseline: &Parsed, current: &Parsed) -> Vec<String> {
    let mut out = Vec::new();

    for (k, want) in &baseline.counts {
        let got = current.counts.get(k).copied().unwrap_or(0);
        if got != *want {
            out.push(format!("count `{k}`: baseline {want}, now {got}"));
        }
    }

    for (id, want) in &baseline.outcomes {
        match current.outcomes.get(id) {
            None => out.push(format!("{id}: was {want}, now passes")),
            Some(got) if got != want => {
                out.push(format!("{id}: was {want}, now {got}"));
            }
            Some(_) => {}
        }
    }
    for (id, got) in &current.outcomes {
        if !baseline.outcomes.contains_key(id) {
            out.push(format!("{id}: was passing, now {got}"));
        }
    }
    out.sort();
    out
}
