// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Floating-point operations that are not available in `core`.
//!
//! `f64::abs`, `is_nan`, `is_infinite` and `is_finite` live in `core`
//! and can be called directly. `floor`, `ceil` and `trunc` do not — they are provided by the platform's libm, which `std` links
//! and a `no_std` target does not.
//!
//! Only these four are shimmed, and that is deliberate. IEEE 754
//! specifies them exactly, so every implementation must agree to the
//! bit. `log10` and `powf` carry no such guarantee — Rust does not
//! specify their precision, and Miri, `libm` and the host disagree by a
//! few ULP on values as ordinary as `17.49`. Depending on either would
//! make `XPath` results differ between a `std` and a `no_std` build, so
//! the evaluator was changed not to need them.
//!
//! `XPath` 1.0 has exactly one numeric type, IEEE 754 double, so these
//! are unavoidable rather than incidental. Routing every call through
//! this module means the `no_std` build breaks *here*, in one file with
//! a clear message, rather than in a dozen places with
//! a `no method named floor` error in a dozen places.

#![allow(clippy::inline_always)]

#[cfg(all(not(feature = "std"), not(feature = "libm")))]
compile_error!(
    "feature `xpath` without `std` requires feature `libm`: XPath needs \
     floor/ceil/trunc, which core does not provide"
);

macro_rules! float_fn {
    ($(#[$m:meta])* $name:ident, $libm:ident) => {
        $(#[$m])*
        #[inline(always)]
        #[must_use]
        pub(crate) fn $name(x: f64) -> f64 {
            #[cfg(feature = "std")]
            { x.$name() }
            #[cfg(all(not(feature = "std"), feature = "libm"))]
            { libm::$libm(x) }
            // Unreachable: the `compile_error!` above already rejects
            // this configuration. It exists only so that the guard is
            // the *only* diagnostic, rather than being buried under a
            // type error from every function in this file.
            #[cfg(all(not(feature = "std"), not(feature = "libm")))]
            { let _ = x; unreachable!() }
        }
    };
}

float_fn!(
    /// Largest integer not greater than `x`.
    floor, floor
);
float_fn!(
    /// Smallest integer not less than `x`.
    ceil, ceil
);
float_fn!(
    /// Integer part, discarding any fraction.
    trunc, trunc
);

#[cfg(test)]
mod tests {
    use super::*;

    /// libm and std must agree **bit for bit**, not approximately.
    ///
    /// Comparing the shim against the inherent methods would be
    /// tautological: with `std` enabled the shim *is* the inherent
    /// method. This calls `libm` directly, so it only means anything
    /// when both features are on — which is why the whole module is
    /// gated on that, and why CI runs `--all-features`.
    ///
    /// If these ever diverge, an `XPath` expression would produce a
    /// different answer on embedded than on the host.
    #[cfg(all(feature = "std", feature = "libm"))]
    #[test]
    fn libm_matches_std_bit_for_bit() {
        let cases = [
            0.0,
            -0.0,
            1.0,
            -1.0,
            0.5,
            -0.5,
            1.5,
            -1.5,
            2.5,
            -2.5,
            1e21,
            -1e21,
            1e-21,
            0.1,
            17.49,
            -17.49,
            1e15,
            4.5,
            -4.5,
            123.456,
            -123.456,
            f64::MAX,
            f64::MIN,
            f64::MIN_POSITIVE,
        ];
        for x in cases {
            assert_eq!(
                libm::floor(x).to_bits(),
                x.floor().to_bits(),
                "floor({x})"
            );
            assert_eq!(
                libm::ceil(x).to_bits(),
                x.ceil().to_bits(),
                "ceil({x})"
            );
            assert_eq!(
                libm::trunc(x).to_bits(),
                x.trunc().to_bits(),
                "trunc({x})"
            );
        }
    }

    #[test]
    fn non_finite_inputs_propagate_rather_than_trapping() {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(floor(x).is_nan(), x.is_nan(), "floor({x})");
            assert_eq!(ceil(x).is_nan(), x.is_nan(), "ceil({x})");
            assert_eq!(trunc(x).is_nan(), x.is_nan(), "trunc({x})");
        }
        assert!(floor(f64::INFINITY).is_infinite());
        assert!(trunc(f64::NEG_INFINITY).is_infinite());
    }
}
