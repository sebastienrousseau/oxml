<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Benchmarks

Method, machine, and how to reproduce. **This file deliberately
publishes no throughput figures yet.** The reason is below, and it is
not modesty.

## Contents

- [Why there are no numbers here](#why-there-are-no-numbers-here)
- [What is measured](#what-is-measured)
- [Allocation counts](#allocation-counts)
- [Running them](#running-them)
- [Recording a figure](#recording-a-figure)
- [How to compare against other parsers](#how-to-compare-against-other-parsers)

## Why there are no numbers here

The same benchmark binary, on the same machine, on the same day,
measured this crate at **14.7 MB/s** and at **123.1 MB/s**. The
difference was the machine's load average, which was above 30 on six
cores.

A throughput figure without its conditions is not a measurement. An
8× spread means any number in that range can be published truthfully
and none of them tells a reader what to expect.

So the rule for this repository is: **no figure is published from a
loaded machine**, and every figure that is published carries the
machine, the toolchain, the load average and the criterion confidence
interval. Until a measurement is taken under those conditions, this
file describes the method and stops there.

The benchmarks themselves run in CI on every push — the job checks they
*compile and run*, which is a different guarantee and one worth having.
It was added after the benchmark suite was found to have been broken
since the first release: `deep_500` exceeded `MAX_DEPTH` and panicked
every time, and nobody noticed because `cargo test` does not build
benches.

## What is measured

| Benchmark | What it isolates |
|---|---|
| `parse` | Document construction, across document shapes and sizes |
| `xpath` | Expression compilation, and evaluation separately from it |

The two are separate because they have different characteristics and
different consumers. Compilation is a one-off; evaluation is the thing
a server repeats. Reporting one number for "XPath" hides which half
you are paying for.

Document shapes matter more than size. A flat document with 100,000
siblings, a deeply nested one, and one dominated by attributes exercise
different parts of the arena, and a single "throughput" number over one
corpus tells you about that corpus.

## Allocation counts

Allocation behaviour **is** published, because it is deterministic and
does not depend on machine load:

| Measurement | Value |
|---|---|
| Allocations per node | **4.13** (66,037 for 16,002 nodes) |
| Allocations for a 1 MB text node | **17** |

Both are held to a ceiling by `crates/oxml/tests/allocations.rs`, which
counts with a wrapping global allocator. The two measurements are
serialised behind a mutex: the counter is global, and when they ran
concurrently they reported each other's allocations and agreed on a
figure that was wrong for both.

The remaining per-node allocations are the owned `String`s — text node
contents, attribute values, and element names before interning.
Removing them requires the document to own its input and store
`(start, len)` ranges into it. That is planned and not done; until it
is, 4.13 is the number.

## Running them

```bash
cargo bench -p oxml
cargo bench -p oxml -- parse       # one group
cargo bench -p oxml -- --save-baseline before
cargo bench -p oxml -- --baseline before
```

Criterion writes an HTML report to `target/criterion/report/index.html`.

## Recording a figure

Before publishing anything from a run:

```bash
uptime                          # load average must be near zero
sysctl -n hw.model hw.ncpu      # the machine
rustc --version                 # the toolchain
```

A published figure states all four, plus criterion's confidence
interval. A delta inside the interval is not a result — a benchmark
delta within noise, cited as a win, is one of the ways a performance
claim becomes untrue without anyone lying.

## How to compare against other parsers

Comparing to `quick-xml` or `roxmltree` fairly is harder than it looks,
because they do not do the same work:

- `quick-xml` is a **pull parser**. It does not build a tree. Comparing
  its throughput to oxml's is comparing tokenisation to tokenisation
  *plus* allocation, interning and namespace resolution.
- `roxmltree` builds a tree but **borrows from the input**, so it does
  not own its strings. That is the design oxml has not yet adopted, and
  it accounts for much of the difference.
- `libxml2` does considerably more — validation, XPath, XSLT — and
  carries the cost of it.

A comparison worth publishing measures a task, not a parser: "extract
every `@href` from this 40 MB document" is a question all four can
answer, and the answer includes the cost of getting the data out, not
only of scanning past it.

See [COMPARISON.md](COMPARISON.md) for the feature-level differences,
which do not depend on a quiet machine.
