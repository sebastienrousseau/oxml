// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Fetch and verify the W3C XML Conformance Test Suite.
//!
//! Uses `curl` and `tar` rather than pulling in an HTTP client and a
//! gzip crate: this runs once, is not published, and adding a
//! dependency tree to a test harness for one download is a poor trade.

use std::path::Path;
use std::process::Command;

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

/// SHA-256, implemented here to keep this crate dependency-free.
fn sha256(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a_2f98,
        0x7137_4491,
        0xb5c0_fbcf,
        0xe9b5_dba5,
        0x3956_c25b,
        0x59f1_11f1,
        0x923f_82a4,
        0xab1c_5ed5,
        0xd807_aa98,
        0x1283_5b01,
        0x2431_85be,
        0x550c_7dc3,
        0x72be_5d74,
        0x80de_b1fe,
        0x9bdc_06a7,
        0xc19b_f174,
        0xe49b_69c1,
        0xefbe_4786,
        0x0fc1_9dc6,
        0x240c_a1cc,
        0x2de9_2c6f,
        0x4a74_84aa,
        0x5cb0_a9dc,
        0x76f9_88da,
        0x983e_5152,
        0xa831_c66d,
        0xb003_27c8,
        0xbf59_7fc7,
        0xc6e0_0bf3,
        0xd5a7_9147,
        0x06ca_6351,
        0x1429_2967,
        0x27b7_0a85,
        0x2e1b_2138,
        0x4d2c_6dfc,
        0x5338_0d13,
        0x650a_7354,
        0x766a_0abb,
        0x81c2_c92e,
        0x9272_2c85,
        0xa2bf_e8a1,
        0xa81a_664b,
        0xc24b_8b70,
        0xc76c_51a3,
        0xd192_e819,
        0xd699_0624,
        0xf40e_3585,
        0x106a_a070,
        0x19a4_c116,
        0x1e37_6c08,
        0x2748_774c,
        0x34b0_bcb5,
        0x391c_0cb3,
        0x4ed8_aa4a,
        0x5b9c_ca4f,
        0x682e_6ff3,
        0x748f_82ee,
        0x78a5_636f,
        0x84c8_7814,
        0x8cc7_0208,
        0x90be_fffa,
        0xa450_6ceb,
        0xbef9_a3f7,
        0xc671_78f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09_e667,
        0xbb67_ae85,
        0x3c6e_f372,
        0xa54f_f53a,
        0x510e_527f,
        0x9b05_688c,
        0x1f83_d9ab,
        0x5be0_cd19,
    ];
    let mut msg = data.to_vec();
    let bit_len = (data.len() as u64) * 8;
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    // `chunks_exact` rather than `as_chunks`: clippy 1.98 prefers the
    // latter, but `slice_as_chunks` is unstable until well after this
    // crate's MSRV of 1.86, so taking the lint's advice breaks the MSRV
    // build. The lint is suppressed rather than obeyed.
    #[allow(clippy::chunks_exact_to_as_chunks)]
    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let b: [u8; 4] = [
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ];
            *word = u32::from_be_bytes(b);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7)
                ^ w[i - 15].rotate_right(18)
                ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17)
                ^ w[i - 2].rotate_right(19)
                ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh] = h;
        for i in 0..64 {
            let s1 =
                e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let t1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 =
                a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (dst, src) in h.iter_mut().zip([a, b, c, d, e, f, g, hh]) {
            *dst = dst.wrapping_add(src);
        }
    }
    h.iter().map(|w| format!("{w:08x}")).collect()
}
