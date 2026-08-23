// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Floating-point operations that are not available in `core`.
//!
//! `f64::abs`, `is_nan`, `is_infinite` and `is_finite` live in `core`
//! and can be called directly. `floor`, `ceil`, `round`, `trunc`,
//! `log10` and `powf` do not — they are provided by the platform's
//! libm, which `std` links and a `no_std` target does not.
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
     floor/ceil/round/trunc/log10/powf, which core does not provide"
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
    /// Nearest integer, ties away from zero.
    ///
    /// Note this is *not* `XPath`'s `round()`, which breaks ties towards
    /// positive infinity. See `xpath_round` in `eval`.
    round, round
);
float_fn!(
    /// Integer part, discarding any fraction.
    trunc, trunc
);
float_fn!(
    /// Base-10 logarithm.
    log10, log10
);

/// `x` raised to the power `y`.
#[inline(always)]
#[must_use]
pub(crate) fn powf(x: f64, y: f64) -> f64 {
    #[cfg(feature = "std")]
    {
        x.powf(y)
    }
    #[cfg(all(not(feature = "std"), feature = "libm"))]
    {
        libm::pow(x, y)
    }
    #[cfg(all(not(feature = "std"), not(feature = "libm")))]
    {
        let _ = (x, y);
        unreachable!()
    }
}

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
                libm::round(x).to_bits(),
                x.round().to_bits(),
                "round({x})"
            );
            assert_eq!(
                libm::trunc(x).to_bits(),
                x.trunc().to_bits(),
                "trunc({x})"
            );
        }
        for x in [1.0, 10.0, 100.0, 0.1, 1e21, 17.49, 3.0] {
            assert_eq!(
                libm::log10(x).to_bits(),
                x.log10().to_bits(),
                "log10({x})"
            );
        }
        for (x, y) in [
            (10.0, 3.0),
            (2.0, 0.5),
            (10.0, -7.0),
            (10.0, 14.0),
            (10.0, 0.0),
        ] {
            assert_eq!(
                libm::pow(x, y).to_bits(),
                x.powf(y).to_bits(),
                "pow({x}, {y})"
            );
        }
    }

    /// The same, for the values `format_number` actually feeds through
    /// `log10`/`powf` when trimming `XPath`'s 15-significant-digit output.
    /// A divergence here changes printed results, not just internals.
    #[cfg(all(feature = "std", feature = "libm"))]
    #[test]
    fn the_number_formatting_path_agrees_between_backends() {
        for n in [17.49_f64, 0.1, 1.0 / 3.0, 1e-7, 123_456_789.123_456, 2.675] {
            let magnitude_std = n.abs().log10().floor();
            let magnitude_libm = libm::floor(libm::log10(n.abs()));
            assert_eq!(
                magnitude_std.to_bits(),
                magnitude_libm.to_bits(),
                "{n}"
            );

            let scale_std = 10f64.powf(14.0 - magnitude_std);
            let scale_libm = libm::pow(10.0, 14.0 - magnitude_libm);
            assert_eq!(scale_std.to_bits(), scale_libm.to_bits(), "{n}");

            assert_eq!(
                (n * scale_std).round().to_bits(),
                libm::round(n * scale_libm).to_bits(),
                "{n}"
            );
        }
    }

    #[test]
    fn non_finite_inputs_propagate_rather_than_trapping() {
        for x in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(floor(x).is_nan(), x.is_nan(), "floor({x})");
            assert_eq!(ceil(x).is_nan(), x.is_nan(), "ceil({x})");
            assert_eq!(round(x).is_nan(), x.is_nan(), "round({x})");
            assert_eq!(trunc(x).is_nan(), x.is_nan(), "trunc({x})");
        }
        assert!(floor(f64::INFINITY).is_infinite());
        assert!(trunc(f64::NEG_INFINITY).is_infinite());
    }
}
