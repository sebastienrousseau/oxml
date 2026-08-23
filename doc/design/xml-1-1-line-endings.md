<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Design note — XML 1.1 line-ending normalisation

**Status:** Diagnosed, not implemented. Tracks the single W3C
conformance test that oxml rejects wrongly.

## The failure

`eduni/rmt-e2e-50` is the only document in the 2,585-test suite that
oxml refuses and should not:

```
<?xml version="1.1" encoding="iso-8859-1"?>
<!DOCTYPE foo [
<!ELEMENT foo ANY>
<!ATTLIST foo bar CDATA #IMPLIED>
]>
<foo\x85bar="hello"/>
```

Byte `0x85` in ISO-8859-1 is U+0085, NEXT LINE. oxml reports
`at byte 120: expected a name`.

## The rule

XML 1.1 §2.11 requires a processor to behave as though it had
normalised, before parsing:

| Sequence | Becomes |
|---|---|
| `#xD #xA` | `#xA` |
| `#xD #x85` | `#xA` |
| `#x85` | `#xA` |
| `#x2028` | `#xA` |
| `#xD` | `#xA` |

XML 1.0 has only the first and last. So in an XML 1.1 document, U+0085
is a line terminator and therefore whitespace — here it separates the
element name from its attribute, and the document is valid.

oxml's whitespace tests are byte-level (`' '`, `'\t'`, `'\r'`, `'\n'`).
U+0085 is two bytes in UTF-8 (`0xC2 0x85`) and U+2028 is three, so
neither is recognised.

## Why it is not a small fix

Normalisation **rewrites the input**. `parse` currently takes a `&str`
and borrows it; producing a normalised copy means either allocating one
per parse or threading a second representation through the parser.

Three options:

1. **Normalise eagerly, always.** Simple, and costs an allocation and a
   copy for every document — including the overwhelming majority that
   contain no `\r` and no NEL at all.
2. **Scan first, normalise only if needed.** One pass to look for the
   affected bytes; allocate only when found. The scan is cheap and
   vectorisable, and most documents pay only the scan.
3. **Handle it in the whitespace and text-accumulation paths.** No
   allocation, but the rule then lives in several places instead of
   one, and `\r\n` inside a text node still has to collapse to `\n`
   somewhere.

**Option 2 is the intended approach**, and it composes with
[ADR 0007](../adr/0007-owned-strings-for-now.md): once the document
owns its input, the normalised copy is what it owns, and the cost
disappears into an allocation that is already happening.

## What to check when implementing

- Offsets. `Error::offset` indexes the string the parser saw. If that
  is a normalised copy, offsets no longer index the caller's input, and
  `line_column` would report positions into a document the caller does
  not have. Either map back, or document the change.
  `tests/properties.rs` asserts offsets land on character boundaries;
  that test should be extended to cover a normalised parse.
- XML 1.0 documents must **not** treat U+0085 as whitespace. The rules
  differ by version, and `Version` is already tracked on the parser.
- The conformance baseline will move by one test. That is a deliberate
  edit with a commit message, by
  [ADR 0006](../adr/0006-baseline-ratchet.md).
