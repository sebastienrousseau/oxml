<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# MSRV and deprecation policy

## Contents

- [Minimum supported Rust version](#minimum-supported-rust-version)
- [How the MSRV is verified](#how-the-msrv-is-verified)
- [When the MSRV will change](#when-the-msrv-will-change)
- [Versioning](#versioning)
- [Deprecation](#deprecation)
- [Feature flags](#feature-flags)

## Minimum supported Rust version

**1.86.0.**

This is a real floor, not an aspiration: CI builds the whole workspace
on exactly 1.86.0 on every push, and the build fails if anything in the
crate needs something newer.

It has held up a change more than once. `slice::as_chunks` is stable
from a later release, and clippy on the CI toolchain suggests it over
`chunks_exact`; taking that advice breaks the MSRV build, so the lint
is suppressed with an explanation rather than obeyed.

## How the MSRV is verified

The MSRV job pins the toolchain at the job level:

```yaml
env:
  RUSTUP_TOOLCHAIN: 1.86.0
```

That is deliberate, and it is there because of a bug. The job installed
1.86.0 and then built on the CI default, because `rust-toolchain.toml`
took precedence over the installed toolchain — so the MSRV job passed
while proving nothing. `RUSTUP_TOOLCHAIN` outranks
`rust-toolchain.toml`, which is what makes the pin real.

The same trap catches local development: a `RUSTUP_TOOLCHAIN` set in
your shell silently overrides the repository's `rust-toolchain.toml`,
and a clippy run that is green locally can be red in CI for that reason
alone.

CI otherwise builds and tests on **1.98.0**.

## When the MSRV will change

A raise is a **minor** version bump, never a patch, and it happens only
when it buys something specific: a borrow-checker improvement that
removes a workaround, or a standard-library function that replaces code
we maintain.

It will not be raised because a newer toolchain exists. Raising the
floor costs every consumer pinned below it, and "the compiler moved on"
is not a benefit to them.

## Versioning

The suite — `oxml`, `xmlschema`, `oxml-cli`, `oxml-mcp`, `oxml-lsp`,
`oxml-wasm` — ships **one version number across every member**, moving
in steps of 0.0.1. `0.1.0` comes after `0.0.999`, not after `0.0.9`.

One number across six crates means a reader never has to work out which
combination is expected to work together. The cost is version churn in
crates that did not change; that is accepted deliberately.

Until 0.1.0, treat every release as capable of breaking API. That is
what `0.0.x` means, and this crate means it.

## Deprecation

Once past 0.1.0:

1. An item to be removed is marked `#[deprecated]` with a note naming
   the replacement.
2. It keeps working for at least one minor release.
3. It is removed in the next breaking release, listed in the changelog.

Anything the compiler cannot warn about — a behavioural change rather
than a signature change — is called out in the release notes rather
than left to be discovered.

## Feature flags

`std`, `xpath` and `libm` are part of the public API. Removing one or
changing what it enables is a breaking change.

Adding a feature is not, provided the default set does not change.
`cargo-hack` builds the feature powerset in CI, so a combination that
does not compile fails there rather than in a consumer's build.
