<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Assurance case

An assurance case is an argument, supported by evidence, that the
software is adequately secure for what it does. This one is
deliberately short: the strongest security claim this project makes is
about what it *cannot* do.

## What this software is

`oxml` is an XML 1.0/1.1 parser, tree and XPath 1.0 engine.

## What it consumes

Its inputs are XML documents, XPath expressions, and DTD internal subsets — all of them untrusted. The threat model assumes every one of them is
hostile: a document written specifically to crash the parser, exhaust
memory, or reach something it should not.

## The claim

**A hostile input can cause this software to return an error. It
cannot cause it to corrupt memory, execute code, exhaust the machine,
or reach the network or the filesystem.**

## The argument

### Memory safety is structural, not tested for

It never opens a file or a socket. External entities are supplied by the caller or not at all, so XXE is foreclosed by construction rather than by a default that can be changed.

### Resource exhaustion is bounded, not merely unlikely

Depth, entity expansion and input size are bounded by explicit limits
with documented defaults. Recursion is bounded because a stack
overflow aborts the process rather than unwinding, and no caller can
catch it.

### Correctness is measured against an external standard

The project does not grade its own homework. Where an independent
conformance suite exists it is run, its denominator is published
alongside its rate, and the result is ratcheted so an unreviewed change
in either direction fails the build.

## The evidence

- `#![forbid(unsafe_code)]`, checked by a CI job that greps for the attribute rather than trusting it is still there.
- 2,557 of 2,557 decided W3C XML Conformance tests pass, with zero panics, gated in CI against a ratcheted baseline that fails on regression *and* on unreviewed improvement.
- Six `cargo-fuzz` targets run on every pull request: parse, parse_limits, stream, tree_walk, xpath_compile, xpath_eval.
- Miri runs on every pull request, checking for undefined behaviour the type system cannot rule out.
- 96.8% line coverage, gated at a 95% floor.
- Entity expansion is bounded per *document*, not per reference: a per-reference budget still admits quadratic blow-up.
- `no_std` builds are verified on three bare-metal targets across every feature combination.

## What this case does *not* claim

- It does not claim the absence of defects. It claims that a defect of
  a particular class — memory corruption — is ruled out by
  construction, and that other classes are bounded and tested for.
- It does not claim the defaults are the tightest possible. They are
  chosen to accept every real document encountered; a service parsing
  untrusted XML under load should tighten them.
- It does not claim independent review. This project has one
  maintainer, and no third party has audited it. That is recorded here
  rather than left to be inferred.

## Reporting a problem with this case

If you can construct an input that violates the claim above, that is a
vulnerability. See [SECURITY.md](../SECURITY.md).
