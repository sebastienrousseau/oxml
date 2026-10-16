// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! SHA-256 archive verification through `RustCrypto`.
//!
//! This verifies a 15 MB archive against a hash pinned in the
//! downloader. A digest function that is subtly wrong does not fail
//! loudly. The maintained primitive is a dependency of this tooling
//! crate alone; the published XML parser gains no runtime dependency.
//! The wrapper lives in the library so its known-answer tests also
//! cover the digest formatting used by the downloader.

/// SHA-256 of `data`, encoded as exactly 64 lowercase hexadecimal digits.
#[must_use]
pub fn sha256(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write as _;
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(data) {
        let _ = write!(hex, "{byte:02x}");
    }
    hex
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` repetitions of `a`, the input the published vectors use.
    fn repeat_a(n: usize) -> Vec<u8> {
        vec![b'a'; n]
    }

    /// The published FIPS 180-2 / RFC 6234 vectors.
    ///
    /// A hand-rolled digest that is never checked against a known
    /// answer is decoration: it produces 64 plausible hex characters
    /// for any input, and the pin it backs is worthless.
    #[test]
    fn matches_the_published_vectors() {
        for (input, want) in [
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
            (
                "abcdefghbcdefghicdefghijdefghijkefghijklfghijklmghijklmn\
                 hijklmnoijklmnopjklmnopqklmnopqrlmnopqrsmnopqrstnopqrstu",
                "cf5b16a778af8380036ce59e7b0492370b249b11e8f07a51afac45037afee9d1",
            ),
        ] {
            assert_eq!(sha256(input.as_bytes()), want, "input {input:?}");
        }
    }

    #[test]
    fn the_message_length_is_appended_as_bits_not_bytes() {
        // One million `a`s: the vector that catches a 32-bit length
        // field, a byte/bit confusion, or a length that overflows.
        let million = repeat_a(1_000_000);
        assert_eq!(
            sha256(&million),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn padding_is_right_at_every_block_boundary() {
        // 55 bytes is the largest message whose padding fits in one
        // block; 56 forces a second one. Getting `while len % 64 != 56`
        // wrong shows up here and almost nowhere else, because every
        // short test vector happens to sit safely inside one block.
        //
        // Expected digests are the published values for `a`-repeated
        // messages at each boundary.
        for (n, want) in [
            (
                55,
                "9f4390f8d30c2dd92ec9f095b65e2b9ae9b0a925a5258e241c9f1e910f734318",
            ),
            (
                56,
                "b35439a4ac6f0948b6d6f9e3c6af0f5f590ce20f1bde7090ef7970686ec6738a",
            ),
            (
                63,
                "7d3e74a05d7db15bce4ad9ec0658ea98e3f06eeecf16b4c6fff2da457ddc2f34",
            ),
            (
                64,
                "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb",
            ),
            (
                65,
                "635361c48bb9eab14198e76ea8ab7f1a41685d6ad62aa9146d301d4f17eb0ae0",
            ),
        ] {
            assert_eq!(sha256(&repeat_a(n)), want, "{n} bytes");
        }
    }

    #[test]
    fn the_digest_is_lowercase_hex_of_exactly_32_bytes() {
        // The pin is compared as a string, so a leading zero dropped by
        // a `{:x}` without a width would silently never match -- or,
        // worse, match the wrong thing.
        for n in [0usize, 1, 31, 32, 100] {
            let d = sha256(&repeat_a(n));
            assert_eq!(d.len(), 64, "{n} bytes produced {d:?}");
            assert!(
                d.chars()
                    .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)),
                "{d:?} is not lowercase hex"
            );
        }
        // A digest whose first byte is 0x6e and whose input is a
        // single NUL: catches a `{:x}` that drops a leading zero from
        // a hex pair, which would shorten the string and never match
        // the pin.
        assert_eq!(
            sha256(&[0u8; 1]),
            "6e340b9cffb37a989ca544e6bb780a2c78901d3fb33738768511a30617afa01d"
        );
    }

    #[test]
    fn a_single_flipped_bit_changes_the_digest() {
        // The property the pin depends on. Without it, a tampered
        // archive of the same length could pass.
        let a = repeat_a(1024);
        let mut b = a.clone();
        b[512] ^= 0x01;
        assert_ne!(sha256(&a), sha256(&b));
    }
}
