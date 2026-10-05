<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Migrating from `quick-xml`

These two are not the same kind of thing, and the first question is
whether you should move at all.

## Should you?

`quick-xml` is a **pull parser**: it hands you events and you build
whatever you need. oxml builds a tree. That difference decides it.

**Stay with `quick-xml` if:**

- Documents can exceed memory. oxml holds the whole tree.
- You touch a small part of a large file — skipping is nearly free in a
  pull parser and costs a full parse in a tree.
- Raw throughput is the metric. `quick-xml` is years ahead and will
  stay there.
- You need to **write** XML. oxml reads only.

**Move to oxml if:**

- You are re-reading the same document, or jumping around it. Every
  random access in a pull parser is another pass.
- You want XPath rather than a hand-written state machine.
- You have written a `match` over `Event::Start` with a depth counter
  and a stack of enum states, and it has grown a bug.

That last one is the honest signal. Reconstructing a tree from events
is a well-known way to spend a week.

## The shape of the change

`quick-xml`, roughly:

```rust,ignore
let mut reader = Reader::from_str(xml);
let mut depth = 0;
let mut in_title = false;
let mut titles = Vec::new();
loop {
    match reader.read_event() {
        Ok(Event::Start(e)) if e.name().as_ref() == b"title" => in_title = true,
        Ok(Event::Text(e)) if in_title => titles.push(e.unescape()?.into_owned()),
        Ok(Event::End(e)) if e.name().as_ref() == b"title" => in_title = false,
        Ok(Event::Eof) => break,
        _ => {}
    }
}
```

oxml:

```rust
use oxml::{XPath, parse};

let xml = "<r><book><title>Dune</title></book><book><title>Germinal</title></book></r>";
let doc = parse(xml).unwrap();
let q = XPath::compile("//title").unwrap();
let titles: Vec<_> = q.evaluate(&doc).nodes().unwrap()
    .iter().map(|&n| doc.text(n)).collect();
assert_eq!(titles, ["Dune", "Germinal"]);
```

The trade is explicit: you paid memory for the tree and got the state
machine back.

## Things that change

| `quick-xml` | oxml |
|---|---|
| `Reader::from_str` | `oxml::parse` |
| `Reader::from_reader` | Read to a `String`, then `parse` |
| `Event::Start` / `End` | The tree already has the nesting |
| `e.unescape()` | Entities are resolved during parsing |
| `e.attributes()` | `doc.attributes(id)` |
| `Writer` | Not available |
| `trim_text(true)` | Not a setting; walk and skip whitespace text |

## Entities and escaping

`quick-xml` gives you raw bytes and you call `unescape` when you want
text. oxml resolves the five predefined entities, numeric character
references and internal general entities during parsing, so
`doc.text(id)` and `attr.value` are already decoded.

External entities are never fetched, which forecloses XXE
structurally. If a document depends on one, that content is silently
absent — see [SECURITY-MODEL.md](SECURITY-MODEL.md).

## Encoding

`quick-xml` has its own encoding handling behind a feature. oxml's
`parse_bytes` reads the BOM and the declaration and supports UTF-8,
UTF-16 both ways and ISO-8859-1. Anything else is
`ErrorKind::UnsupportedEncoding` — decode it yourself and call `parse`.
