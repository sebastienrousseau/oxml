<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Benchmarks

Method, machine, and how to reproduce. **This file deliberately
publishes no absolute throughput figures yet** — no MB/s. The reason
is below, and it is not modesty.

It does publish **ratios** against `quick-xml` and `roxmltree`, which
survive the conditions an absolute cannot. See
[Comparing against other parsers](#comparing-against-other-parsers).

## Contents

- [Why there are no numbers here](#why-there-are-no-numbers-here)
- [What is measured](#what-is-measured)
- [Allocation counts](#allocation-counts)
- [Running them](#running-them)
- [Recording a figure](#recording-a-figure)
- [Comparing against other parsers](#comparing-against-other-parsers)

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
| `throughput` | Bytes per second, across markup-, text- and attribute-dominated documents |
| `encoding` | Decoding, which runs before parsing on every `parse_bytes` |
| `tree` | Reading a parsed document, which an XPath-free caller still pays |
| `entities` | Expansion, against a control with the same output and no entities |

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
| Allocations per node | **0.50** (8,076 for 16,004 nodes) |
| Allocations for a 1 MB text node | **17** |

Both are held to a ceiling by `crates/oxml/tests/allocations.rs`, which
counts with a wrapping global allocator. The two measurements are
serialised behind a mutex: the counter is global, and when they ran
concurrently they reported each other's allocations and agreed on a
figure that was wrong for both.

It was 4.13, then 1.13 once child lists were flattened and names
interned by borrowed parts. The step to 0.50 is the document owning
its input: text nodes, comments and attribute values are `(start,
len)` ranges into it, so a document that expands no entities allocates
nothing at all for its character data.

What is left is the arenas themselves — the node, child, attribute and
name vectors — which grow by doubling and so cost a handful of
allocations for the whole document rather than one per node.

## Running them

```bash
cargo bench -p oxml
cargo bench -p oxml -- parse       # one group
cargo bench -p oxml -- --save-baseline before
cargo bench -p oxml -- --baseline before
```

Criterion writes an HTML report to `target/criterion/report/index.html`.

## Recording a figure

Use `scripts/record-throughput.sh`. It checks the conditions below
before it measures and **exits without a number when they are not
met**, so a figure cannot be recorded from a busy machine by
forgetting to look.

Manually, before publishing anything from a run:

```bash
uptime                          # load average must be near zero
sysctl -n hw.model hw.ncpu      # the machine
rustc --version                 # the toolchain
```

A published figure states all four, plus criterion's confidence
interval. A delta inside the interval is not a result — a benchmark
delta within noise, cited as a win, is one of the ways a performance
claim becomes untrue without anyone lying.

## Comparing against other parsers

Comparing to `quick-xml` or `roxmltree` fairly is harder than it looks,
because they do not do the same work:

- `quick-xml` is a **pull parser**. It does not build a tree. Comparing
  its throughput to `oxml::parse` is comparing tokenisation to
  tokenisation *plus* allocation, interning and namespace resolution.
  Since [`oxml::stream`](../crates/oxml/src/stream.rs) exists there is
  a like-for-like counterpart, and that is what is compared.
- `roxmltree` builds a tree but **borrows from the input**, so it does
  not own its strings. That is the design oxml has not yet adopted, and
  it accounts for much of the difference. It is the substance of the
  comparison, not an unfairness in it.
- `libxml2` does considerably more — validation, XPath, XSLT — and
  carries the cost of it. It is not benchmarked here.

`benches/comparison.rs` measures the two fair pairings. Both crates
are dev-dependencies, so the numbers can be regenerated rather than
believed:

```sh
cargo bench --bench comparison
cargo bench --bench comparison -- --check   # fail on regression
```

Recorded 2026-08-26, 6-core Mac17,5, rustc 1.98.0, 855 KB catalogue,
median of six runs at load averages between 8 and 15:

| Group | oxml | Reference | Ratio |
|---|---|---|---|
| events, no tree | `oxml::stream` | `quick-xml` | **0.089×** |
| tree | `oxml::parse` | `roxmltree` | **0.319×** |

### Why a ratio works where an absolute does not

An absolute figure is a property of the machine as much as of the
code. A ratio is much less so: when two implementations parse the same
document while the same processes compete for the same cores,
contention slows both, and what it does to their quotient is far
smaller than what it does to either term.

That only holds if they are measured *together*. Criterion measures
each benchmark in its own block, seconds apart, so a load spike
between blocks lands on one arm and not the other — precisely the
error the ratio is meant to remove. So this harness pairs them: within
a round every implementation parses the same document back to back,
milliseconds apart.

Pairing alone is not enough either, and the measurement says so.
Preemption is not proportional: a scheduler quantum lands on whichever
arm is running when it falls, so the slower arm absorbs more of them.
With ten CPU hogs on six cores, a median-of-ratios estimate of the
`events` group **halved**, from 0.096 to 0.054, because `quick-xml` is
roughly ten times faster on this document and preemption did not
divide evenly between them.

The reported figure is therefore the ratio of the **fastest** run of
each arm. Contention can only make a run slower, never faster, so the
fastest observed run is the best available estimate of the uncontended
cost and the sample the fewest quanta landed on. Under the same ten
hogs, that estimator moved by 3% and 5%:

| | quiet | 10 CPU hogs | drift |
|---|---|---|---|
| `oxml::stream` (fastest-run) | 0.096× | 0.091× | 5% |
| `oxml::parse` (fastest-run) | 0.322× | 0.313× | 3% |
| median-of-pairs, for contrast | 0.096× | 0.048× | halved |

The median-of-pairs is still printed, as a diagnostic: when it sits
well below the reported ratio, the machine was contended while
measuring.

### What the check does and does not catch

`--check` fails if a ratio falls more than **15%** below its recorded
baseline. That band comes from measurement, not taste: six runs on a
loaded machine spread the ratios about ±8% around their median, so a
tighter tolerance would fail on noise.

The cost is worth stating plainly — this catches regressions of
roughly a fifth or worse, not of a twentieth. A quiet machine would
support a tighter band.

Baselines are keyed by architecture, because instruction mix, cache
sizes and memory bandwidth move two parsers differently. That is not a
hypothetical: measured on a GitHub `ubuntu-latest` runner against the
6-core Mac,

| Group | aarch64 | x86_64 |
|---|---|---|
| `oxml::parse` vs `roxmltree` | 0.319× | 0.321× |
| `oxml::stream` vs `quick-xml` | 0.089× | **0.113×** |

the tree ratio is near-identical while the events ratio is 27% better
on x86_64. A single shared baseline would have been vacuous on one
machine or spurious on the other.

An architecture with no recorded baseline reports its numbers and does
not gate, so adding a platform cannot produce a red build before
anyone has measured it there.

### Still missing

A task-level comparison: "extract every `@href` from this 40 MB
document" is a question all four can answer, and the answer includes
the cost of getting the data out, not only of scanning past it. The
ratios above measure parsing, which is the cost before any of that.

See [COMPARISON.md](COMPARISON.md) for the feature-level differences,
which do not depend on a quiet machine.
