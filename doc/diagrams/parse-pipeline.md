<!-- SPDX-License-Identifier: Apache-2.0 OR MIT -->

# Diagrams

Mermaid sources, kept in version control so they can be reviewed as
text.

## The parse pipeline

```mermaid
flowchart TD
    bytes["&[u8]"] --> decode["encoding::decode<br/>BOM, then declaration"]
    str["&str"] --> parser
    decode -->|"Cow&lt;str&gt;<br/>UTF-8 borrowed"| parser["parser::parse_with<br/>recursive descent"]
    decode -->|malformed| encerr["MalformedEncoding<br/>UnsupportedEncoding"]
    parser --> dtd["dtd::parse<br/>internal subset only"]
    dtd --> parser
    parser --> limits{"Limits"}
    limits -->|exceeded| err["Error + byte offset"]
    limits -->|within| tree["Document<br/>arena of nodes"]
    tree --> xpath["XPath::evaluate"]
```

## The document arena

```mermaid
flowchart LR
    subgraph Document
        nodes["nodes: Vec&lt;Node&gt;"]
        names["names: Vec&lt;ExpandedName&gt;<br/>interned"]
        child["child_ids: Vec&lt;NodeId&gt;<br/>every child list, concatenated"]
        attr["attr_ids: Vec&lt;NodeId&gt;"]
    end
    nodes -->|"NameId"| names
    nodes -->|"(start, len)"| child
    nodes -->|"(start, len)"| attr
```

A `Node` stores ranges rather than owning a `Vec` each. That is the
difference between one allocation per node and one per node *plus* one
per child list.

## Where a document can be refused

```mermaid
flowchart TD
    input["document"] --> enc{"decodes?"}
    enc -->|no| e1["MalformedEncoding"]
    enc -->|"legal name, unknown"| e2["UnsupportedEncoding"]
    enc -->|yes| wf{"well-formed?"}
    wf -->|no| e3["the ErrorKind for the rule broken"]
    wf -->|yes| dep{"nesting within max_depth?"}
    dep -->|no| e4["DepthLimitExceeded"]
    dep -->|yes| ent{"entity budget intact?"}
    ent -->|no| e5["EntityLimitExceeded"]
    ent -->|yes| size{"within size bounds?"}
    size -->|no| e6["TooManyNodes, TextTooLong, …"]
    size -->|yes| ok["Document"]
```
