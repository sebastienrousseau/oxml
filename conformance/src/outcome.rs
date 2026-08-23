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

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(
        pass: usize,
        fail: usize,
        unsupported: usize,
        blocked: usize,
        panic: usize,
    ) -> Counts {
        Counts {
            pass,
            fail,
            unsupported,
            blocked,
            panic,
        }
    }

    #[test]
    fn skipping_a_hard_test_cannot_raise_the_pass_rate() {
        // This is the honesty property of the whole metric. If
        // `Unsupported` fed the pass-rate denominator the wrong way, a
        // runner could reclassify every failure it disliked and watch
        // its score climb. Adding unsupported tests must move coverage
        // down and leave the pass rate exactly where it was.
        let before = counts(9, 1, 0, 0, 0);
        let after = counts(9, 1, 90, 0, 0);
        assert!((before.pass_rate() - after.pass_rate()).abs() < f64::EPSILON);
        assert!(
            after.coverage() < before.coverage(),
            "hiding 90 tests must show up as lost coverage: {} vs {}",
            after.coverage(),
            before.coverage()
        );
    }

    #[test]
    fn blocked_tests_are_excluded_the_same_way_unsupported_ones_are() {
        let a = counts(3, 1, 0, 0, 0);
        let b = counts(3, 1, 0, 40, 0);
        assert!((a.pass_rate() - b.pass_rate()).abs() < f64::EPSILON);
        assert!(b.coverage() < a.coverage());
    }

    #[test]
    fn a_panic_counts_against_the_pass_rate_rather_than_being_set_aside() {
        // A panic is a defect, not an unimplemented feature. Were it
        // excluded like `Unsupported`, a parser that aborted on every
        // hard input would score 100%.
        let c = counts(1, 0, 0, 0, 1);
        assert_eq!(c.decided(), 2);
        assert!((c.pass_rate() - 50.0).abs() < f64::EPSILON);
        assert!((c.coverage() - 100.0).abs() < f64::EPSILON);
    }

    #[test]
    fn totals_account_for_every_outcome() {
        let mut c = Counts::default();
        let all = [
            Outcome::Pass,
            Outcome::Fail,
            Outcome::Unsupported,
            Outcome::Blocked,
            Outcome::Panic,
        ];
        for o in all {
            c.add(o);
        }
        assert_eq!(c.total(), all.len(), "a variant is not being tallied");
        assert_eq!(c, counts(1, 1, 1, 1, 1));
        assert_eq!(c.decided(), 3);
    }

    #[test]
    fn is_decided_agrees_with_the_decided_tally() {
        // Two independent statements of the same rule; they drift
        // apart if only one is updated when a variant is added.
        for o in [
            Outcome::Pass,
            Outcome::Fail,
            Outcome::Unsupported,
            Outcome::Blocked,
            Outcome::Panic,
        ] {
            let mut c = Counts::default();
            c.add(o);
            assert_eq!(
                o.is_decided(),
                c.decided() == 1,
                "{o} disagrees with itself"
            );
        }
    }

    #[test]
    fn an_empty_run_reports_zero_rather_than_nan() {
        // `0/0` is NaN, and NaN compares false against every threshold,
        // so a ratchet built on it would silently accept anything.
        let c = Counts::default();
        assert_eq!(c.total(), 0);
        assert!((c.pass_rate() - 0.0).abs() < f64::EPSILON);
        assert!((c.coverage() - 0.0).abs() < f64::EPSILON);
        assert!(c.pass_rate().is_finite() && c.coverage().is_finite());
    }

    #[test]
    fn outcomes_order_by_severity_so_a_run_summarises_to_its_worst() {
        assert!(Outcome::Pass < Outcome::Unsupported);
        assert!(Outcome::Unsupported < Outcome::Blocked);
        assert!(Outcome::Blocked < Outcome::Fail);
        assert!(Outcome::Fail < Outcome::Panic);

        let run = [Outcome::Pass, Outcome::Panic, Outcome::Fail, Outcome::Pass];
        assert_eq!(run.into_iter().max(), Some(Outcome::Panic));
    }

    #[test]
    fn every_outcome_prints_a_distinct_stable_name() {
        // These strings are the baseline file's on-disk format; a
        // change here silently invalidates every recorded baseline.
        let names: Vec<_> = [
            Outcome::Pass,
            Outcome::Unsupported,
            Outcome::Blocked,
            Outcome::Fail,
            Outcome::Panic,
        ]
        .iter()
        .map(ToString::to_string)
        .collect();
        assert_eq!(names, ["pass", "unsupported", "blocked", "fail", "panic"]);
    }

    #[test]
    fn the_summary_line_shows_both_numerators_and_both_denominators() {
        // A pass rate without its denominator is how a thin run passes
        // for a thorough one, so the Display impl must carry both.
        let text = counts(9, 1, 5, 2, 0).to_string();
        for part in [
            "9 pass",
            "1 fail",
            "5 unsupported",
            "2 blocked",
            "90.0%",
            "10",
            "58.8%",
            "17",
        ] {
            assert!(text.contains(part), "{part:?} missing from {text:?}");
        }
    }
}
