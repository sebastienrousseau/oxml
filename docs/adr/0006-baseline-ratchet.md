# 0006 — Fail on unreviewed conformance improvement

**Status:** Accepted

## Context

The W3C suite has 2,585 tests and the parser does not pass all of them.
A plain threshold ("fail under 93%") catches regressions and nothing
else.

## Decision

`conformance/baselines/w3c-xml.tsv` records the expected outcome of
every individual test. The run fails when any test changes outcome —
including a test that starts **passing**.

## Consequences

Failing on improvement reads as pedantry until it catches something.
When a batch of tests suddenly passes, the likeliest cause is not that
the parser improved; it is that the runner stopped running them
properly.

That is not hypothetical. The catalogue loader keyed on the wrong
element and silently dropped all 159 Sun tests, then reported a
confident pass rate for the remaining 2,426. Nothing in the output
looked wrong. It was caught by an assertion on the total test count.

Updating the baseline is a deliberate act that shows up in a diff, with
a commit message saying which tests moved and why.

## Related

The same reasoning drove keeping `Unsupported` distinct from `Fail`,
and reporting the pass rate always beside its coverage figure. Twice a
reclassification was found that would have pushed the headline past
95%; both were reverted, because both raised the number without
changing what the parser does.
