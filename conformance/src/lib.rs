// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! W3C XML Conformance Test Suite runner.
//!
//! # Why this is a separate crate
//!
//! It is not published, it downloads 15 MB of third-party data, and it
//! is the only part of the repository that needs a network. Keeping it
//! out of `oxml` means none of that reaches a consumer.
//!
//! # Why the suite is downloaded rather than vendored
//!
//! The tarball ships **no LICENSE file** — the terms live only in the
//! W3C FAQ — and James Clark's `xmltest/` portion explicitly forbids
//! redistribution in modified form. Downloading at test time avoids the
//! question entirely.
//!
//! # Why per-collection manifests rather than `xmlconf.xml`
//!
//! The top-level `xmlconf.xml` pulls each submission in through an
//! *external entity reference*, so reading it needs a parser that
//! resolves external entities — which `oxml` deliberately does not do.
//! It also carries a known defect: the `eduni-misc` entity is wrapped in
//! `xml:base="eduni/namespaces/misc/"`, a directory that does not
//! exist, so a runner honouring `xml:base` correctly loses all nine of
//! the 2013 release's new tests. Reading each collection manifest in
//! place sidesteps both.

pub mod baseline;
pub mod catalog;
pub mod outcome;
pub mod runner;
pub mod sha256;

use std::path::{Path, PathBuf};

/// The suite release this runner targets.
///
/// Stated explicitly because it matters: **libxml2 and Expat both still
/// pin `xmlts20080827`**, the 2008 release. The two disagree on three
/// tests' expected outcomes, so "passes the W3C suite" means nothing
/// without a version.
pub const SUITE_RELEASE: &str = "xmlts20130923";

/// Where the tarball comes from.
pub const SUITE_URL: &str = "https://www.w3.org/XML/Test/xmlts20130923.tar.gz";

/// SHA-256 of the tarball, verified on download.
pub const SUITE_SHA256: &str =
    "9b61db9f5dbffa545f4b8d78422167083a8568c59bd1129f94138f936cf6fc1f";

/// Total `<TEST` elements in the 2013 release, **including** one that
/// sits inside an XML comment in `ibm/xml-1.1/ibm_not-wf.xml`.
///
/// Both numbers get quoted in the wild. 2,586 is the raw occurrence
/// count; [`REAL_TESTS`] is how many are actually tests.
pub const RAW_TEST_OCCURRENCES: usize = 2586;

/// Tests the runner should see: 2,586 minus the commented-out one.
pub const REAL_TESTS: usize = 2585;

/// Root of the downloaded suite, or `None` if it is not present.
#[must_use]
pub fn data_dir() -> Option<PathBuf> {
    let d = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("data")
        .join("xmlconf");
    d.is_dir().then_some(d)
}

/// Skip a test cleanly when the suite has not been downloaded.
///
/// Fetching 15 MB from the network on every `cargo test` would be rude,
/// so the suite is opt-in via `OXML_CONFORMANCE=1` or a prior
/// `cargo run --bin download`.
#[macro_export]
macro_rules! require_suite {
    () => {
        match $crate::data_dir() {
            Some(d) => d,
            // Set `OXML_REQUIRE_SUITE=1` to make a missing suite a
            // failure instead of a skip.
            //
            // `cargo test` on a fresh clone has no network and should
            // not need one, so the default is to skip. But a skip is
            // reported as a pass, and `scripts/gate.sh` exists to be
            // "everything CI runs, locally" -- so on a fresh clone the
            // gate printed `conformance ok` having run **zero** of the
            // 2,585 tests. That is the failure this repository keeps
            // finding: a check that appears to run and does not.
            //
            // The gate and CI set the variable. A bare `cargo test`
            // does not, and still skips.
            None if ::std::env::var_os("OXML_REQUIRE_SUITE").is_none() => {
                eprintln!(
                    "conformance suite not present; run \
                     `cargo run -p oxml-conformance --bin download` \
                     (skipping)"
                );
                return;
            }
            None => panic!(
                "OXML_REQUIRE_SUITE is set and the conformance suite is \
                 not present. Run `cargo run -p oxml-conformance --bin \
                 download`. Refusing to report a pass for tests that \
                 did not run."
            ),
        }
    };
}
