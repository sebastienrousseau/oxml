// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! What happened to one test.

use core::fmt;

/// The result of running a single test case.
///
/// Ordered by severity so a run can be summarised by its worst outcome.
/// The distinction that matters is between **Fail** — we got the answer
/// wrong — and **Unsupported** — we do not implement the feature the
/// test exercises. Collapsing the two either flatters the score or
/// buries real defects, depending on which way you collapse it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    /// The parser agreed with the suite.
    Pass,
    /// The test exercises something not implemented — XML 1.1, an
    /// encoding other than UTF-8. Counted separately and excluded from
    /// the pass rate, but reported in coverage so the denominator is
    /// visible.
    Unsupported,
    /// The harness could not decide — a missing file, for instance.
    /// Two tests in the suite reference files that were never shipped.
    Blocked,
    /// The parser disagreed with the suite.
    Fail,
    /// The parser panicked. Always the worst outcome: a caller cannot
    /// catch it, and it means an input can take down a process.
    Panic,
}

impl Outcome {
    /// Whether this outcome counts towards the pass rate denominator.
    #[must_use]
    pub const fn is_decided(self) -> bool {
        matches!(self, Self::Pass | Self::Fail | Self::Panic)
    }
}

impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pass => "pass",
            Self::Unsupported => "unsupported",
            Self::Blocked => "blocked",
            Self::Fail => "fail",
            Self::Panic => "panic",
        })
    }
}

/// Tallies for a whole run.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Counts {
    /// Agreed with the suite.
    pub pass: usize,
    /// Disagreed.
    pub fail: usize,
    /// Feature not implemented.
    pub unsupported: usize,
    /// Harness could not decide.
    pub blocked: usize,
    /// Panicked.
    pub panic: usize,
}

impl Counts {
    /// Record one outcome.
    pub fn add(&mut self, o: Outcome) {
        match o {
            Outcome::Pass => self.pass += 1,
            Outcome::Fail => self.fail += 1,
            Outcome::Unsupported => self.unsupported += 1,
            Outcome::Blocked => self.blocked += 1,
            Outcome::Panic => self.panic += 1,
        }
    }

    /// Every test seen.
    #[must_use]
    pub const fn total(&self) -> usize {
        self.pass + self.fail + self.unsupported + self.blocked + self.panic
    }

    /// Tests where the parser gave a definite answer.
    #[must_use]
    pub const fn decided(&self) -> usize {
        self.pass + self.fail + self.panic
    }

    /// Pass rate over decided tests, as a percentage.
    ///
    /// Always report this **with** [`Counts::coverage`]. A high rate on
    /// a thin denominator is how a runner flatters itself: skip
    /// everything hard and 100% is easy.
    #[must_use]
    pub fn pass_rate(&self) -> f64 {
        if self.decided() == 0 {
            return 0.0;
        }
        self.pass as f64 * 100.0 / self.decided() as f64
    }

    /// Share of all tests that produced a definite answer.
    #[must_use]
    pub fn coverage(&self) -> f64 {
        if self.total() == 0 {
            return 0.0;
        }
        self.decided() as f64 * 100.0 / self.total() as f64
    }
}

impl fmt::Display for Counts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} pass, {} fail, {} panic, {} unsupported, {} blocked \
             — {:.1}% of {} decided ({:.1}% coverage of {})",
            self.pass,
            self.fail,
            self.panic,
            self.unsupported,
            self.blocked,
            self.pass_rate(),
            self.decided(),
            self.coverage(),
            self.total(),
        )
    }
}
