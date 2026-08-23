// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Fetch and verify the W3C XML Conformance Test Suite.
//!
//! Uses `curl` and `tar` rather than pulling in an HTTP client and a
//! gzip crate: this runs once, is not published, and adding a
//! dependency tree to a test harness for one download is a poor trade.

use std::path::Path;
use std::process::Command;

use oxml_conformance::sha256::sha256;

use oxml_conformance::{SUITE_RELEASE, SUITE_SHA256, SUITE_URL};

fn main() -> Result<(), String> {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    let data = manifest.join("data");
    if data.join("xmlconf").is_dir() {
        println!("already present: {}", data.join("xmlconf").display());
        return Ok(());
    }
    std::fs::create_dir_all(&data).map_err(|e| e.to_string())?;
    let tarball = data.join(format!("{SUITE_RELEASE}.tar.gz"));

    println!("downloading {SUITE_URL}");
    // The User-Agent is load-bearing, not cargo-culting. www.w3.org is
    // behind Cloudflare: a request without a browser UA gets a 5,850
    // byte HTML challenge page with HTTP 200. Piping that into `tar`
    // yields an empty directory, and a conformance job then reports
    // success having run zero tests.
    let status = Command::new("curl")
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--user-agent",
            "Mozilla/5.0 (compatible; oxml-conformance/0.0; \
             +https://github.com/sebastienrousseau/oxml)",
            "--output",
        ])
        .arg(&tarball)
        .arg(SUITE_URL)
        .status()
        .map_err(|e| format!("could not run curl: {e}"))?;
    if !status.success() {
        return Err(format!("curl failed: {status}"));
    }

    // Verify before extracting. This catches the challenge page, a
    // truncated transfer, and any upstream change to a supposedly
    // frozen 2013 release.
    let bytes = std::fs::read(&tarball).map_err(|e| e.to_string())?;
    let digest = sha256(&bytes);
    if digest != SUITE_SHA256 {
        let _ = std::fs::remove_file(&tarball);
        return Err(format!(
            "checksum mismatch for {SUITE_RELEASE}\n  expected {SUITE_SHA256}\n  \
             got      {digest}\n  ({} bytes — a few KB usually means a \
             Cloudflare challenge page rather than the tarball)",
            bytes.len()
        ));
    }
    println!("verified sha256 {digest}");

    let status = Command::new("tar")
        .arg("xzf")
        .arg(&tarball)
        .arg("-C")
        .arg(&data)
        .status()
        .map_err(|e| format!("could not run tar: {e}"))?;
    if !status.success() {
        return Err(format!("tar failed: {status}"));
    }
    let _ = std::fs::remove_file(&tarball);
    println!("extracted to {}", data.join("xmlconf").display());
    Ok(())
}
