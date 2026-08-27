// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! How oxml compares to `quick-xml` and `roxmltree`, as a **ratio**.
//!
//! # Why a ratio, and why not criterion
//!
//! An absolute figure in MB/s is a property of the machine as much as
//! of the code: the same binary measured 14.7 and 123.1 MB/s on one
//! host on one day, and the difference was load. That is why
//! `scripts/record-throughput.sh` refuses to record when the machine
//! is busy — and why, on a machine that is never quiet, it records
//! nothing at all.
//!
//! A ratio survives what an absolute cannot. If two parsers run the
//! same document while the same processes compete for the same cores,
//! contention slows both, and what it does to their *quotient* is
//! very much smaller than what it does to either term.
//!
//! That only holds if the two are measured *together*. Criterion
//! measures each benchmark in its own block, seconds apart, so a load
//! spike between blocks lands on one arm and not the other — exactly
//! the error the ratio is supposed to remove. So this harness pairs
//! them instead: within a round, every implementation parses the same
//! document back to back, milliseconds apart, and the round yields one
//! ratio per implementation. The reported figure is the median across
//! rounds, with the interquartile range as its spread.
//!
//! # Comparing like with like
//!
//! `quick-xml` is a pull parser and builds no tree; `roxmltree` builds
//! a tree that borrows the input. Timing oxml's *tree* against
//! `quick-xml`'s *event scan* compares two different jobs, which is
//! what the README did before [`oxml::stream`] existed. Now there are
//! two honest groups:
//!
//! - **events** — [`oxml::stream::Reader`] against `quick-xml`, both
//!   yielding events and building nothing.
//! - **tree** — [`oxml::parse`] against `roxmltree`, both building a
//!   navigable tree. oxml's owns its strings and `roxmltree`'s borrows
//!   them, which is the substance of the difference rather than an
//!   unfairness in the measurement.
//!
//! ```sh
//! cargo bench --bench comparison
//! cargo bench --bench comparison -- --check   # fail on regression
//! ```

#![allow(missing_docs)]

use core::fmt::Write as _;
use std::hint::black_box;
use std::time::{Duration, Instant};

/// Rounds to run. Each yields one ratio per implementation.
const ROUNDS: usize = 60;

/// Rounds actually run, allowing an override.
///
/// The reported figure is the *fastest* run, so it is a bound that can
/// only improve as rounds are added: more samples cannot make the
/// minimum worse, only reveal a less-contended one. Sixty is enough to
/// rank two parsers against each other, which is what this harness is
/// for. Converging on an absolute throughput takes more, and
/// `OXML_BENCH_ROUNDS` is how you ask for it.
fn rounds() -> usize {
    std::env::var("OXML_BENCH_ROUNDS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(ROUNDS)
}

/// Rounds discarded before measuring, to let caches and the branch
/// predictor reach steady state.
const WARMUP: usize = 10;

/// How far a ratio may fall below its recorded baseline before
/// `--check` fails.
///
/// Fifteen percent, chosen from measurement rather than taste: six
/// runs on a loaded machine spread the ratios about ±8% around their
/// median, so a tolerance below that would fail on noise. It is still
/// far inside a real regression -- an allocation added per node, or a
/// borrow turned into a copy, moves these by much more.
///
/// The cost of the wider band is honest to state: this catches
/// regressions of roughly a fifth or worse, not of a twentieth. A
/// quiet machine would support a tighter one.
const TOLERANCE: f64 = 0.15;

/// Ratios recorded on a known-good build, as
/// `(arch, group, name, ratio)`.
///
/// Keyed by architecture because a ratio is a property of the code
/// *and* the machine it runs on: instruction mix, cache sizes and
/// memory bandwidth all move two parsers differently. A baseline from
/// Apple Silicon is not evidence about an x86 runner.
///
/// An architecture with no entry reports its numbers and never fails,
/// so adding a platform cannot produce a spurious red build. Fill one
/// in only from a measured run on that platform.
///
/// A ratio *above* its baseline is an improvement and never fails; the
/// check is one-sided. Update these deliberately, never to make a red
/// build green.
/// Recorded 2026-08-26 on a 6-core Mac17,5 under rustc 1.98.0, as the
/// median of six runs at load averages between 8 and 15 -- which is
/// the point of a ratio: an absolute figure taken there would be
/// meaningless, and `scripts/record-throughput.sh` rightly refuses to
/// record one.
const BASELINE: &[(&str, &str, &str, f64)] = &[
    ("aarch64", "events", "oxml::stream", 0.089),
    ("aarch64", "tree", "oxml::parse", 0.319),
    // From two CI runs on a GitHub ubuntu-latest runner, taking the
    // lower of each. Worth noting why this table is keyed at all: the
    // *tree* ratio is near-identical across the two architectures
    // (0.319 against 0.321), while the *events* ratio is 27% better on
    // x86_64 (0.089 against 0.113). One shared baseline would have
    // been vacuous on one machine or spurious on the other.
    ("x86_64", "events", "oxml::stream", 0.113),
    ("x86_64", "tree", "oxml::parse", 0.321),
];

/// A catalogue: the shape most XML in the wild actually has.
fn catalogue(items: usize) -> String {
    let mut s =
        String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<catalogue>");
    for i in 0..items {
        let _ = write!(
            s,
            "<item id=\"i{i}\" sku=\"S{i:06}\">\
             <name>Product number {i}</name>\
             <price currency=\"GBP\">{}.{:02}</price>\
             <description>A description of product {i}, long enough to \
             be representative of real character data.</description>\
             </item>",
            i % 500,
            i % 100
        );
    }
    s.push_str("</catalogue>");
    s
}

/// One timed run of one implementation.
type Arm = (&'static str, fn(&str) -> usize);

fn oxml_events(input: &str) -> usize {
    use oxml::stream::{Event, Reader};
    let mut reader = Reader::new(input).expect("well-formed");
    let mut n = 0usize;
    while let Some(event) = reader.next_event().expect("well-formed") {
        if matches!(event, Event::StartElement { .. }) {
            n += 1;
        }
    }
    n
}

fn quick_xml_events(input: &str) -> usize {
    use quick_xml::Reader;
    use quick_xml::events::Event;
    let mut reader = Reader::from_str(input);
    let mut n = 0usize;
    loop {
        match reader.read_event().expect("well-formed") {
            Event::Start(_) | Event::Empty(_) => n += 1,
            Event::Eof => break,
            _ => {}
        }
    }
    n
}

fn oxml_tree(input: &str) -> usize {
    let doc = oxml::parse(input).expect("well-formed");
    doc.len()
}

fn roxmltree_tree(input: &str) -> usize {
    let doc = roxmltree::Document::parse(input).expect("well-formed");
    doc.descendants().count()
}

/// Time one arm once, returning elapsed time and its result.
fn time(arm: Arm, input: &str) -> (Duration, usize) {
    let start = Instant::now();
    let out = black_box(arm.1(black_box(input)));
    (start.elapsed(), out)
}

/// The median of a slice, which is not sorted in place for the caller.
fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).expect("no NaN in a duration"));
    let mid = v.len() / 2;
    if v.len() % 2 == 0 {
        f64::midpoint(v[mid - 1], v[mid])
    } else {
        v[mid]
    }
}

/// Run one group and report each arm's ratio against the reference.
///
/// The reference is the last arm, and is the implementation the ratio
/// is *against* -- so a ratio below 1.0 means slower than it.
///
/// # Why the minimum and not the median
///
/// Pairing the arms removes load that is *proportional*. Preemption is
/// not proportional: a scheduler quantum lands on whichever arm is
/// running when it falls, so the arm that takes longer absorbs more of
/// them. Measured here against `quick-xml`, which is roughly ten times
/// faster on this document, ten competing CPU hogs moved the
/// median-of-ratios from 0.100 to 0.054 -- it halved, while the `tree`
/// group, whose arms are within 3x of each other, barely moved.
///
/// The minimum does not have that failure. Contention can only make a
/// run slower, never faster, so the fastest observed run is the best
/// available estimate of the uncontended cost, and it is the sample
/// that the fewest quanta landed on. It is the standard estimator for
/// exactly this reason.
/// The one-minute load average per core, if it can be read.
///
/// Used to decide whether a verdict is worth giving at all. The
/// median-of-pairs was tried first as a contention signal and only
/// catches one direction: under twelve CPU hogs the `tree` group's
/// median sat *above* its minimum-based ratio, so nothing looked
/// wrong while the ratio had collapsed from 0.32 to 0.17. The load
/// average measures the condition directly instead of inferring it,
/// which is what `scripts/record-throughput.sh` already does for
/// absolute figures.
fn load_per_core() -> Option<f64> {
    // Captured once, before any measuring. The benchmark is itself a
    // CPU load: reading this *after* a run reported 4.07 per core on a
    // machine that had been sitting at 2.34, so the measurement
    // supplied the evidence that it could not be trusted and
    // suppressed its own verdict.
    static AT_START: std::sync::OnceLock<Option<f64>> =
        std::sync::OnceLock::new();
    *AT_START.get_or_init(read_load_per_core)
}

/// Read the one-minute load average per core.
fn read_load_per_core() -> Option<f64> {
    let out = std::process::Command::new("uptime").output().ok()?;
    let text = String::from_utf8(out.stdout).ok()?;
    // Both spellings, because they differ by platform and getting it
    // wrong fails *silently*: this returned `None` on macOS, where
    // `uptime` writes "load averages: 72.93 60.98 65.40" with spaces,
    // while the parser expected Linux's "load average: 0.5, 0.4, 0.3"
    // with commas. The gate then never fired and looked like it was
    // there.
    let after = text.split("average").nth(1)?;
    let first = after
        .trim_start_matches([':', 's', ' '])
        .split([',', ' '])
        .find_map(|t| t.trim().parse::<f64>().ok())?;
    let cores = std::thread::available_parallelism().ok()?.get();
    Some(first / cores as f64)
}

/// One measurement of a group: every arm's per-round times.
fn measure(
    name: &str,
    input: &str,
    arms: &[Arm],
    rounds: usize,
) -> Vec<Vec<f64>> {
    let mut samples: Vec<Vec<f64>> = vec![Vec::new(); arms.len()];
    let mut found: Vec<usize> = vec![0; arms.len()];

    // Equalise how long each timed segment lasts.
    //
    // This is what made the `events` group fragile. Preemption lands
    // on whichever arm is running when a quantum falls, so an arm that
    // runs ten times longer absorbs ten times as many -- a *systematic*
    // bias, not noise, which is why measuring again never cleared it.
    // `quick-xml` is roughly ten times faster than `oxml::stream` on
    // this document, and that group read anywhere from 0.065 to 0.093
    // against a 0.089 baseline. The `tree` group, whose arms are within
    // three times of each other, stayed put.
    //
    // Running the faster arm enough times to match the slower one
    // makes the segments comparable, so a quantum is equally likely to
    // land on either. The per-iteration time is what is recorded, so
    // the ratio still means the same thing.
    let calibration: Vec<f64> = arms
        .iter()
        .map(|arm| time(*arm, input).0.as_secs_f64())
        .collect();
    let slowest = calibration.iter().copied().fold(0.0_f64, f64::max);
    let reps: Vec<usize> = calibration
        .iter()
        .map(|d| {
            if *d <= 0.0 {
                1
            } else {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss
                )]
                let n = (slowest / d).round() as usize;
                n.clamp(1, 64)
            }
        })
        .collect();

    for round in 0..(rounds + WARMUP) {
        for (i, arm) in arms.iter().enumerate() {
            let start = std::time::Instant::now();
            let mut n = 0;
            for _ in 0..reps[i] {
                n = black_box(arm.1(black_box(input)));
            }
            let elapsed = start.elapsed();
            if round >= WARMUP {
                // Per iteration, so the ratio is unchanged by how many
                // were needed to fill the segment.
                #[allow(clippy::cast_precision_loss)]
                samples[i].push(elapsed.as_secs_f64() / reps[i] as f64);
            }
            found[i] = n;
        }
    }

    // Every arm must have done the same work, or the ratio compares
    // two different jobs. `quick-xml` reports empty elements
    // separately, and `roxmltree` counts nodes rather than elements,
    // so these are counts of comparable things, not identical ones.
    for (i, arm) in arms.iter().enumerate() {
        assert!(found[i] > 0, "{name}: {} found nothing", arm.0);
    }

    samples
}

/// The fastest time in each arm's samples.
fn minima(samples: &[Vec<f64>]) -> Vec<f64> {
    samples
        .iter()
        .map(|v| v.iter().copied().fold(f64::INFINITY, f64::min))
        .collect()
}

/// Run one group and report each arm's ratio against the reference.
///
/// The reference is the last arm, and is the implementation the ratio
/// is *against* -- so a ratio below 1.0 means slower than it.
///
/// # Why the minimum and not the median
///
/// Pairing the arms removes load that is *proportional*. Preemption is
/// not proportional: a scheduler quantum lands on whichever arm is
/// running when it falls, so the arm that takes longer absorbs more of
/// them. Measured here against `quick-xml`, which is roughly ten times
/// faster on this document, ten competing CPU hogs moved the
/// median-of-ratios from 0.100 to 0.054 -- it halved, while the `tree`
/// group, whose arms are within 3x of each other, barely moved.
///
/// The minimum does not have that failure. Contention can only make a
/// run slower, never faster, so the fastest observed run is the best
/// available estimate of the uncontended cost, and it is the sample
/// that the fewest quanta landed on.
///
/// # Why a suspected regression is measured again
///
/// The estimator is good, not perfect: at about five load per core
/// this printed `REGRESSED` against a tree that had not changed. That
/// is worse than a missing check, because a gate that cries wolf gets
/// ignored and then misses the real thing.
///
/// Contention can only bias the ratio **downward** -- it inflates both
/// arms' minima and the slower arm's more -- so a low reading may be
/// noise while a high one cannot be. The best of several attempts is
/// therefore the trustworthy estimate, and only a regression that
/// survives every attempt is reported. A real one survives: the code
/// cannot get faster between attempts.
fn group(name: &str, input: &str, arms: &[Arm], check: bool) -> bool {
    /// Attempts before a regression is believed.
    const ATTEMPTS: usize = 3;

    /// Load per core above which no verdict is given.
    ///
    /// Deliberately high, and the reason is the failure mode on the
    /// other side. A gate that suppresses often is worse than no gate:
    /// it turns the check off without saying so, and a real regression
    /// then passes silently. Set at 1.5 first, this refused to judge a
    /// *genuine* regression on a machine sitting at 2.7 per core --
    /// which is where this one idles.
    ///
    /// Equalising the timed segments is what actually made the
    /// measurement robust; this is only a backstop for conditions
    /// where nothing could be measured. With the segments equal, four
    /// runs under twelve CPU hogs on six cores all stayed above their
    /// floors. Four per core is past that and into the range where the
    /// numbers stopped meaning anything.
    ///
    /// `scripts/record-throughput.sh` wants 0.20 per core for an
    /// *absolute* figure. A ratio tolerates twenty times more, which
    /// is the whole argument for publishing ratios.
    const MAX_LOAD: f64 = 4.0;

    let reference = *arms.last().expect("a group needs a reference");
    let last = arms.len() - 1;
    let arch = std::env::consts::ARCH;
    let baseline = |arm: &str| {
        BASELINE
            .iter()
            .find(|(a, g, n, _)| *a == arch && *g == name && *n == arm)
            .map(|(_, _, _, base)| *base)
    };

    let mut samples = measure(name, input, arms, rounds());
    let mut best = minima(&samples);
    let mut attempts = 1;

    // Repeat only if something looks wrong, so a healthy run costs one
    // measurement.
    while check
        && attempts < ATTEMPTS
        && arms[..last].iter().enumerate().any(|(i, arm)| {
            baseline(arm.0).is_some_and(|base| {
                base > 0.0 && best[last] / best[i] < base * (1.0 - TOLERANCE)
            })
        })
    {
        attempts += 1;
        println!(
            "  {name}: a ratio is below its floor -- measuring again \
             ({attempts} of {ATTEMPTS}) to tell a regression from a \
             busy machine"
        );
        let again = measure(name, input, arms, rounds());
        // Accumulated, not replaced: every round is a sample of the
        // same quantity, so a later attempt adds evidence rather than
        // superseding it. The minimum over all of them is the least
        // contended run seen, and cannot get worse.
        for (all, more) in samples.iter_mut().zip(again) {
            all.extend(more);
        }
        best = minima(&samples);
    }

    let reference_min = best[last];

    // Throughput as well as ratio, from the same samples. An absolute
    // figure normally needs a quiet machine -- the same binary
    // measured 14.7 and 123.1 MB/s here on one day -- but that is a
    // property of the *estimator*, not of absolutes. See
    // `doc/BENCHMARKS.md`.
    let mb = |secs: f64| input.len() as f64 / secs / 1_000_000.0;
    println!(
        "\n{name}  ({} KB, vs {} at {:.2} ms = {:.0} MB/s)",
        input.len() / 1024,
        reference.0,
        reference_min * 1e3,
        mb(reference_min)
    );

    let mut ok = true;
    let mut contended = false;
    for (i, arm) in arms[..last].iter().enumerate() {
        let mine = best[i];
        let ratio = reference_min / mine;
        // The median of the paired ratios, as a contention
        // diagnostic: when it sits well below the reported ratio the
        // machine was busy while measuring, and the minimum-based
        // figure is the one to believe.
        let median_ratio = median(
            &samples[i]
                .iter()
                .zip(&samples[last])
                .map(|(m, t)| t / m)
                .collect::<Vec<_>>(),
        );
        print!(
            "  {:<14} {ratio:.3}x  ({:.2} ms = {:>4.0} MB/s)   \
             median-of-pairs {median_ratio:.3}x",
            arm.0,
            mine * 1e3,
            mb(mine)
        );
        match baseline(arm.0) {
            Some(base) if base > 0.0 => {
                let floor = base * (1.0 - TOLERANCE);
                if ratio >= floor {
                    print!("  baseline {base:.3}x");
                } else if load_per_core().is_some_and(|l| l > MAX_LOAD) {
                    // Below the floor, but the machine was too busy
                    // for the reading to mean anything. Say so and do
                    // not fail: a gate that cries wolf on a loaded
                    // runner gets ignored, and then it misses the real
                    // regression. "Cannot tell" is a different answer
                    // from "no regression", and this prints it.
                    let load = load_per_core().unwrap_or(0.0);
                    print!(
                        "  BELOW FLOOR but the machine is busy \
                         ({load:.2} load per core, limit {MAX_LOAD:.1}); \
                         not judged"
                    );
                    contended = true;
                } else {
                    print!(
                        "  REGRESSED after {attempts} attempts \
                         (baseline {base:.3}x, floor {floor:.3}x)"
                    );
                    ok = false;
                }
            }
            Some(_) => print!("  no baseline recorded"),
            None => print!("  no baseline for {arch}"),
        }
        println!();
    }
    if contended {
        println!(
            "  {name}: measured on a busy machine, so a low ratio here \
             is not evidence of a regression. Re-run when quiet."
        );
    }
    if check { ok } else { true }
}

fn main() {
    let check = std::env::args().any(|a| a == "--check");
    let doc = catalogue(4_000);

    println!(
        "paired ratio benchmark -- {} rounds after {WARMUP} warmup\n\
         each round times every arm back to back on the same document,\n\
         so contention lands on both and the quotient survives it.\n\
         load now: {}",
        rounds(),
        std::process::Command::new("uptime")
            .output()
            .ok()
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .map_or_else(
                || "unknown".to_owned(),
                |s| s.split("age").last().unwrap_or("?").trim().to_owned(),
            )
    );

    // Read before anything is measured, so the figure describes the
    // machine rather than this process.
    let _ = load_per_core();

    let events = group(
        "events",
        &doc,
        &[
            ("oxml::stream", oxml_events),
            ("quick-xml", quick_xml_events),
        ],
        check,
    );
    let tree = group(
        "tree",
        &doc,
        &[("oxml::parse", oxml_tree), ("roxmltree", roxmltree_tree)],
        check,
    );

    if check && !(events && tree) {
        eprintln!(
            "\nA ratio fell more than {:.0}% below its baseline.",
            TOLERANCE * 100.0
        );
        std::process::exit(1);
    }
}
