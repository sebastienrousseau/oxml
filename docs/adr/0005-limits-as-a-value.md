# 0005 — `Limits` is a value, not a builder

**Status:** Accepted

## Context

Ten bounds need to reach the parser. The usual options are a builder, a
config struct, or global/thread-local state.

## Decision

A plain `Copy` struct passed to `parse_with`. No global state, no
builder, no environment variable.

## Consequences

- Two documents on two threads can use different limits.
- A `Limits` can live in an application's configuration struct, be
  compared, logged, or serialised by the caller.
- The whole policy is visible in one place instead of spread across a
  call chain.
- `parse` uses `Limits::default()`, so the common case is one call.

`Limits` is `#[non_exhaustive]`. Adding a bound is then not a breaking
change for callers, but it does mean the struct cannot be built with
literal syntax from outside the crate. The supported pattern is to
start from a profile and assign:

```rust,ignore
let mut limits = Limits::default();
limits.max_depth = 32;
```

That is slightly awkward and it is the price of being able to add a
bound later without a major version.

## A profile that deliberately does not do what its name says

`permissive()` raises nine of the ten bounds and leaves `max_depth`
at the default.

Depth is the one bound whose cost is stack rather than heap, and a
stack overflow **aborts the process** rather than unwinding, so no
caller can catch it. Raising it to 10,000 was tried; the test process
aborted. Measured by binary search across subprocesses, a 2 MiB thread
stack overflows at around 280 levels in a debug build, at roughly
7,489 bytes per frame.

A permissive profile that crashes is not permissive. The field
documents this so the next person to "fix the inconsistency" reads the
reason first.
