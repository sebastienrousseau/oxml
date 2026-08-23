# 0002 — `#![forbid(unsafe_code)]`, at a cost

**Status:** Accepted

## Context

The fastest XML parsers use `unsafe`: unchecked indexing in hot loops,
SIMD scanning, transmuting validated bytes to `str`. Each is defensible
and each is a place a bug becomes memory corruption.

## Decision

`#![forbid(unsafe_code)]` in `lib.rs`. Not `deny`, which a module can
override; `forbid`, which nothing inside the crate can.

CI additionally greps for the attribute, so removing it is a visible
line in a diff rather than a quiet change that still compiles.

## Consequences

**Gained**

Memory-corruption bugs are ruled out categorically rather than argued
about per review. For a crate whose input arrives from the network in
every one of its front ends, that is the property worth having.

**Given up**

Throughput. `quick-xml` is years ahead and some of that gap is
techniques unavailable here. The README says so rather than implying
the crate is fast in a way it is not.

## What it does not buy

The attribute prevents memory unsafety. It does not prevent:

- Panics — bounded by fuzzing, property tests and a conformance suite
  that records **zero** panics across 2,585 documents.
- Resource exhaustion — bounded by `Limits`.
- Logic errors — bounded by tests, and by the conformance number being
  published with its denominator.

Tests may use `unsafe` where there is no alternative:
`tests/allocations.rs` implements a counting global allocator, which
cannot be written safely. That is a separate crate from the library and
does not weaken the library's guarantee.
