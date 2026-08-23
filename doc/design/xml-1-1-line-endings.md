<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Design note — XML 1.1 line-ending normalisation

**Status:** Implemented. Kept as the record of what the change had
to get right.

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

## Why it looked harder than it was

Normalisation **rewrites the input**, and the note originally recorded
this as blocked on the owned-input change, on the assumption that a
normalised copy could not outlive the call.

That was wrong. `Document` owns every string it holds and has no
lifetime parameter, so parsing from a temporary `String` is fine --
the tree does not borrow from it. The borrowing rewrite that would
have made this hard has not happened, so the change is local.

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

**Option 2 is what was implemented.** `normalize_line_endings` scans
for a terminator and returns `Cow::Borrowed` when there is none, which
is every document written on a Unix-like system. Only a document that
actually contains a carriage return, NEL or LINE SEPARATOR pays for a
copy.

When the owned-input change lands
([ADR 0007](../adr/0007-owned-strings-for-now.md)), the normalised copy
becomes the thing the document owns and the allocation disappears into
one that is already happening.

## What the fix had to get right

- **Offsets.** `Error::offset` indexes the string the parser saw, which
  is the normalised copy. Line and column survive it: every removed
  `\r` sits immediately before the `\n` it pairs with, so the number of
  `\n` before any point is unchanged and the column counts from the
  same place. A lone `\r`, or a NEL in 1.1, *becomes* a line break --
  and the reported line is then the one the specification says it is.
- **Character references.** `&#xD;` is markup when normalisation runs,
  so it survives and expands to a carriage return afterwards. It is the
  only way to write one, and normalising it would make the character
  unrepresentable.
- **Version differences.** U+0085 is a line ending in 1.1 and an
  ordinary character in 1.0. Treating it as whitespace in 1.0 would
  accept documents the specification says are malformed, so
  `<a\u{85}b="h"/>` is still an error without a 1.1 declaration.
- **The whole entity, not just text.** Comments and CDATA are
  normalised too, because the rule applies before parsing.
- **The baseline.** It moved by one test, as a deliberate edit, by
  [ADR 0006](../adr/0006-baseline-ratchet.md).

## What it also fixed

The note was written about XML 1.1 because that was the failing
conformance test. Running the cases first showed that **XML 1.0
normalisation was missing as well**: `<a>x\r\ny</a>` returned
`"x\r\ny"` where the specification requires `"x\ny"`.

That affects every document written on Windows, and nothing in the
conformance suite caught it -- the suite tests whether a document is
accepted, not what its text turns out to be.
