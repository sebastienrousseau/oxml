// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Running the suite against `oxml`.

use crate::catalog::{TestCase, TestType};
use crate::outcome::{Counts, Outcome};

/// One scored test.
#[derive(Debug, Clone)]
pub struct Scored {
    /// The test's `ID`.
    pub id: String,
    /// Which submission it came from.
    pub submission: String,
    /// What happened.
    pub outcome: Outcome,
    /// Why, when that is not obvious. Informational only — never
    /// compared against a baseline, because error wording changes far
    /// more often than behaviour does.
    pub detail: String,
}

/// Why a test is not applicable to this parser.
///
/// Every one of these is a feature `oxml` does not claim, and each is
/// listed in the README so the number is honest rather than quietly
/// improving the pass rate.
fn unsupported_reason(case: &TestCase) -> Option<&'static str> {
    // Namespaces 1.1 adds undeclaration of prefixes, which is not
    // implemented. XML 1.1 itself now is.
    if case
        .recommendation
        .as_deref()
        .is_some_and(|r| r.starts_with("NS1.1"))
    {
        return Some("namespaces-1.1");
    }
    if !case.namespace {
        return Some("namespace-processing-off");
    }
    None
}

/// Score one test case.
#[must_use]
pub fn run_one(case: &TestCase) -> Scored {
    let scored = |outcome, detail: String| Scored {
        id: case.id.clone(),
        submission: case.submission.clone(),
        outcome,
        detail,
    };

    let bytes = match std::fs::read(&case.path) {
        Ok(b) => b,
        // Two tests reference files that were never shipped —
        // `ibm49i02.dtd` is the documented one. Blocked, not failed:
        // the parser was never asked anything.
        Err(e) => {
            return scored(Outcome::Blocked, format!("unreadable: {e}"));
        }
    };
    if let Some(reason) = unsupported_reason(case) {
        return scored(Outcome::Unsupported, reason.to_owned());
    }

    // `catch_unwind` so one panicking input reports as a panic rather
    // than ending the whole run. A panic is the worst outcome available
    // and must be visible per-test.
    // The edition a test applies to determines which name rules are
    // correct for it. The suite ships complementary pairs — the same
    // name is not-well-formed under editions 1-4 and well-formed under
    // the 5th — so a parser fixed to one edition cannot be scored
    // against the other's tests at all. `oxml` implements both and the
    // runner selects, which is what makes those 309 tests decidable
    // rather than merely skipped.
    let mut limits = oxml::Limits::default();
    if case
        .edition
        .as_deref()
        .is_some_and(|ed| !ed.split_whitespace().any(|e| e == "5"))
    {
        limits.edition = oxml::Edition::Fourth;
    }
    let parsed =
        std::panic::catch_unwind(|| oxml::parse_bytes_with(&bytes, limits));
    let Ok(parsed) = parsed else {
        return scored(Outcome::Panic, "panicked".to_owned());
    };
    // An encoding this crate cannot decode is out of scope rather than
    // wrong. A *malformed* encoding name is not — that is a
    // well-formedness error and is scored normally.
    if matches!(
        parsed.as_ref().err().map(|e| &e.kind),
        Some(oxml::ErrorKind::UnsupportedEncoding)
    ) {
        return scored(Outcome::Unsupported, "unsupported-encoding".to_owned());
    }

    // `TYPE="error"` marks a condition the suite's own DTD says a
    // parser *may* report. Both outcomes conform, so both are a pass —
    // what must not happen is a panic, and that is caught above. This
    // is scored rather than skipped because "we did not crash on it" is
    // a real result, and leaving it undecided understates coverage.
    if case.kind == TestType::Error {
        return scored(Outcome::Pass, "optional-error-test".to_owned());
    }

    match (case.kind, parsed) {
        // Must accept, and did.
        (TestType::Valid, Ok(_)) => scored(Outcome::Pass, String::new()),
        (TestType::Valid, Err(e)) => {
            scored(Outcome::Fail, format!("rejected a valid document: {e}"))
        }
        // Must reject, and did.
        (TestType::NotWellFormed, Err(_)) => {
            scored(Outcome::Pass, String::new())
        }
        (TestType::NotWellFormed, Ok(_)) => scored(
            Outcome::Fail,
            "accepted a document that is not well-formed".to_owned(),
        ),
        // A non-validating parser is *permitted* to accept these, and
        // `oxml` does not validate. Accepting is the correct outcome;
        // rejecting means we found it not well-formed, which is wrong.
        (TestType::Invalid, Ok(_)) => scored(Outcome::Pass, String::new()),
        (TestType::Invalid, Err(e)) => scored(
            Outcome::Fail,
            format!("invalid document treated as not well-formed: {e}"),
        ),
        (TestType::Error, _) => unreachable!("handled above"),
    }
}

/// Score the whole suite.
#[must_use]
pub fn run_all(cases: &[TestCase]) -> (Vec<Scored>, Counts) {
    let mut counts = Counts::default();
    let mut out = Vec::with_capacity(cases.len());
    for case in cases {
        let scored = run_one(case);
        counts.add(scored.outcome);
        out.push(scored);
    }
    (out, counts)
}
