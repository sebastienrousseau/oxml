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
fn unsupported_reason(
    case: &TestCase,
    source: Option<&str>,
) -> Option<&'static str> {
    if let Some(rec) = case.recommendation.as_deref() {
        if rec.starts_with("XML1.1") {
            return Some("xml-1.1");
        }
        if rec.starts_with("NS1.1") {
            return Some("namespaces-1.1");
        }
    }
    if case.version.as_deref() == Some("1.1") {
        return Some("xml-1.1");
    }
    // `EDITION` lists the XML 1.0 editions a test applies to. A test
    // that applies only to the 5th edition exercises the relaxed name
    // rules, which this parser does not implement.
    if case.edition.as_deref().is_some_and(|ed| {
        !ed.split_whitespace()
            .any(|e| matches!(e, "1" | "2" | "3" | "4"))
    }) {
        return Some("xml-1.0-5th-edition-only");
    }
    if !case.namespace {
        return Some("namespace-processing-off");
    }
    // Only UTF-8 is implemented, so anything with another encoding or a
    // non-UTF-8 BOM is out of scope rather than wrong.
    if let Some(src) = source {
        let head = &src.as_bytes()[..src.len().min(128)];
        if head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]) {
            return Some("utf-16-bom");
        }
        if let Some(enc) = declared_encoding(src) {
            let e = enc.to_ascii_lowercase();
            if !matches!(e.as_str(), "utf-8" | "us-ascii" | "ascii") {
                return Some("non-utf-8-encoding");
            }
        }
    }
    None
}

fn declared_encoding(src: &str) -> Option<String> {
    let decl = src.strip_prefix("<?xml")?;
    let end = decl.find("?>")?;
    let decl = &decl[..end];
    let at = decl.find("encoding")?;
    let rest = &decl[at + "encoding".len()..];
    let rest = rest.trim_start().strip_prefix('=')?.trim_start();
    let quote = rest.chars().next()?;
    if quote != '"' && quote != '\'' {
        return None;
    }
    let rest = &rest[1..];
    let close = rest.find(quote)?;
    Some(rest[..close].to_owned())
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

    // `TYPE="error"` is optional to report per the suite's own DTD.
    if case.kind == TestType::Error {
        return scored(Outcome::Unsupported, "optional-error-test".to_owned());
    }

    let bytes = match std::fs::read(&case.path) {
        Ok(b) => b,
        // Two tests reference files that were never shipped —
        // `ibm49i02.dtd` is the documented one. Blocked, not failed:
        // the parser was never asked anything.
        Err(e) => {
            return scored(Outcome::Blocked, format!("unreadable: {e}"));
        }
    };
    let Ok(source) = String::from_utf8(bytes) else {
        return scored(Outcome::Unsupported, "non-utf-8-bytes".to_owned());
    };

    if let Some(reason) = unsupported_reason(case, Some(&source)) {
        return scored(Outcome::Unsupported, reason.to_owned());
    }

    // `catch_unwind` so one panicking input reports as a panic rather
    // than ending the whole run. A panic is the worst outcome available
    // and must be visible per-test.
    let parsed = std::panic::catch_unwind(|| oxml::parse(&source));
    let Ok(parsed) = parsed else {
        return scored(Outcome::Panic, "panicked".to_owned());
    };

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
