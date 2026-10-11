// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! One construct per call, through the tree parser's own scanner.
//!
//! The dispatch mirrors `parse_children` and `parse_document` arm for
//! arm, so the two agree on what a document contains. What differs is
//! what is kept: the tree parser keeps an arena, this keeps the names
//! of the open elements and the state a later construct may need --
//! the namespaces in scope, the entities the DOCTYPE declared, the
//! expansion budget.

use alloc::borrow::{Cow, ToOwned};
use alloc::string::String;
use alloc::vec::Vec;

use super::event::BorrowedEvent;
use crate::error::{Error, ErrorKind, Result};
use crate::parser::{Parser, Run, Version};
use crate::tree::{Chars, ExpandedName};
use crate::{Document, Limits};

/// Parser state that outlives a single event.
#[derive(Debug)]
pub(super) struct Carried {
    document: Document,
    namespaces: crate::parser::Namespaces,
    names: alloc::collections::BTreeMap<String, Vec<u32>>,
    version: Version,
    pub(super) limits: Limits,
    depth: usize,
    /// Declarations from the internal subset.
    ///
    /// Carried because the `DOCTYPE` is consumed by the first event
    /// and its entities are referenced by later ones.
    dtd: Option<crate::dtd::Dtd>,
    /// Whether the document declared `standalone="yes"`.
    standalone: bool,
    /// What is left of the document's entity-expansion budget.
    ///
    /// Per document, not per event. Rebuilding it each scan handed a
    /// bomb split across fifty text nodes fifty full budgets.
    entity_budget: usize,
    /// Validated entity replacement texts, keyed by entity name and
    /// the namespace bindings active when referenced.
    entity_cache: alloc::collections::BTreeSet<(String, u64)>,
}

impl Carried {
    pub(super) fn new(limits: Limits) -> Self {
        Self {
            document: Document::placeholder(),
            namespaces: crate::parser::Namespaces::default(),
            names: alloc::collections::BTreeMap::new(),
            version: Version::V10,
            limits,
            depth: 0,
            dtd: None,
            standalone: false,
            entity_budget: limits.max_entity_expansion,
            entity_cache: alloc::collections::BTreeSet::new(),
        }
    }

    /// What the XML declaration settled.
    pub(super) fn declare(&mut self, version: Version, standalone: bool) {
        self.version = version;
        self.standalone = standalone;
    }

    /// A parser over `text` holding this state, to be given back by
    /// [`Self::reclaim`].
    fn lend<'a>(
        &mut self,
        text: &'a str,
        pos: usize,
        external: &'a dyn crate::external::ExternalSource,
    ) -> Parser<'a> {
        Parser {
            input: text,
            bytes: text.as_bytes(),
            pos,
            doc: core::mem::replace(
                &mut self.document,
                Document::placeholder(),
            ),
            name_index: core::mem::take(&mut self.names),
            external,
            ns: core::mem::take(&mut self.namespaces),
            depth: self.depth,
            limits: self.limits,
            dtd: self.dtd.take(),
            version: self.version,
            entity_budget: self.entity_budget,
            entity_depth: 0,
            standalone: self.standalone,
            entity_cache: core::mem::take(&mut self.entity_cache),
        }
    }

    /// Take the state back from a parser [`Self::lend`] made.
    fn reclaim(&mut self, mut parser: Parser<'_>) {
        self.depth = parser.depth;
        self.document =
            core::mem::replace(&mut parser.doc, Document::placeholder());
        self.names = core::mem::take(&mut parser.name_index);
        self.namespaces = core::mem::take(&mut parser.ns);
        self.dtd = parser.dtd.take();
        self.entity_budget = parser.entity_budget;
        self.entity_cache = core::mem::take(&mut parser.entity_cache);
    }
}

/// Where the scan has reached in the document.
#[derive(Debug)]
pub(super) struct Cursor {
    /// Scanner position, carried between events.
    pub(super) pos: usize,
    /// Names of the elements currently open, innermost last.
    pub(super) open: Vec<(String, ExpandedName)>,
    /// Whether the XML declaration has been consumed.
    started: bool,
    /// Whether the one permitted root element has been seen.
    seen_root: bool,
    /// Still inside the run of `DOCTYPE`s the tree parser's
    /// `skip_prolog` consumes without objection: every one that
    /// directly follows the declaration, separated by whitespace
    /// alone. Only a `DOCTYPE` met after a comment or processing
    /// instruction is checked against an earlier one, so the reader
    /// does the same, or it would refuse a document `parse` accepts.
    prolog_doctypes: bool,
    /// What the tree parser's arena would hold by now.
    nodes: NodeTally,
}

impl Cursor {
    pub(super) const fn new() -> Self {
        Self {
            pos: 0,
            open: Vec::new(),
            started: false,
            seen_root: false,
            prolog_doctypes: true,
            nodes: NodeTally::new(),
        }
    }
}

/// A count of the nodes the tree parser would have created, for
/// [`Limits::max_nodes`].
///
/// The reader builds no arena, so the bound the tree parser applies
/// to its arena has to be applied to a count of what the arena would
/// hold: the document node, every element with its attribute and
/// namespace nodes, every text run, comment and processing
/// instruction. Counted the same way and checked at the same point,
/// so the two entry points refuse the same document at the same
/// offset -- which is what the agreement tests hold them to.
#[derive(Debug)]
struct NodeTally {
    /// Nodes so far, the document node included.
    count: usize,
    /// A text run has been yielded but not yet counted.
    ///
    /// The tree parser accumulates a run and flushes it into a node
    /// when the next construct arrives, *after* checking the limit
    /// for that construct. Counting the run when it is yielded would
    /// trip the limit one event early.
    pending_text: bool,
}

impl NodeTally {
    const fn new() -> Self {
        Self {
            count: 1,
            pending_text: false,
        }
    }

    /// The check the tree parser makes before each child, then the
    /// flush of any text run it had been accumulating.
    fn check(&mut self, limit: Option<usize>, at: usize) -> Result<()> {
        if limit.is_some_and(|m| self.count > m) {
            return Err(Error::new(ErrorKind::TooManyNodes, at));
        }
        if self.pending_text {
            self.pending_text = false;
            self.count += 1;
        }
        Ok(())
    }

    fn add(&mut self, nodes: usize) {
        self.count += nodes;
    }

    fn text(&mut self) {
        self.pending_text = true;
    }
}

/// The end event a self-closing tag owes, held until the next pull.
#[derive(Debug)]
pub(super) struct Pending {
    name: ExpandedName,
    prefix: Option<String>,
}

impl Pending {
    pub(super) fn into_event<'a>(self) -> BorrowedEvent<'a> {
        BorrowedEvent::EndElement {
            name: self.name,
            prefix: self.prefix.map(Cow::Owned),
        }
    }
}

/// What one turn of the scanner produced.
pub(super) enum Scanned<'a> {
    Event(BorrowedEvent<'a>),
    /// A self-closing tag, which is two events.
    SelfClosed(BorrowedEvent<'a>, Pending),
    Eof,
}

/// Run `f` over a parser on `text` at the cursor, moving the carried
/// state into the parser for the duration and back out again.
///
/// The alternative is a self-referential struct: the parser borrows
/// `text`, which the caller owns.
fn with_parser<'a, T>(
    text: &'a str,
    carried: &mut Carried,
    cursor: &mut Cursor,
    external: &'a dyn crate::external::ExternalSource,
    f: impl FnOnce(&mut Parser<'a>, &mut Cursor) -> Result<T>,
) -> Result<T> {
    let mut parser = carried.lend(text, cursor.pos, external);
    let result = f(&mut parser, cursor);
    cursor.pos = parser.pos;
    carried.reclaim(parser);
    result
}

/// Scan the construct at the cursor: an event, or the end.
pub(super) fn scan<'a>(
    text: &'a str,
    carried: &mut Carried,
    cursor: &mut Cursor,
    external: &'a dyn crate::external::ExternalSource,
) -> Result<Scanned<'a>> {
    with_parser(text, carried, cursor, external, one_event)
}

/// Consume a construct at the cursor that yields no event -- the XML
/// declaration, or a `DOCTYPE` outside the root -- and say whether
/// there was one.
///
/// Kept apart from [`scan`] because what follows such a construct may
/// not have arrived yet. The caller checks that before scanning on;
/// a loop inside the scanner took an empty buffer for the end of the
/// document.
pub(super) fn skip_eventless<'a>(
    text: &'a str,
    carried: &mut Carried,
    cursor: &mut Cursor,
    external: &'a dyn crate::external::ExternalSource,
) -> Result<bool> {
    with_parser(text, carried, cursor, external, eventless_step)
}

fn eventless_step(
    parser: &mut Parser<'_>,
    cursor: &mut Cursor,
) -> Result<bool> {
    if !cursor.started {
        declaration(parser)?;
        cursor.started = true;
        return Ok(true);
    }
    if !cursor.open.is_empty() {
        return Ok(false);
    }
    parser.skip_whitespace();
    if parser.starts_with("<!DOCTYPE") {
        doctype(parser, cursor)?;
        return Ok(true);
    }
    if parser.pos < parser.bytes.len() {
        // Anything else outside the root ends the run of prolog
        // `DOCTYPE`s that `skip_prolog` would have consumed.
        cursor.prolog_doctypes = false;
    }
    Ok(false)
}

/// Read whichever construct comes next.
fn one_event<'a>(
    parser: &mut Parser<'a>,
    cursor: &mut Cursor,
) -> Result<Scanned<'a>> {
    // Outside the root element the grammar is not element content
    // but `Misc*`: comments, processing instructions and whitespace,
    // around exactly one element.
    if cursor.open.is_empty() {
        return outside_root(parser, cursor);
    }
    // Where the tree parser checks its arena: before each child.
    cursor.nodes.check(parser.limits.max_nodes, parser.pos)?;
    if parser.pos >= parser.bytes.len() {
        return Err(Error::new(ErrorKind::UnexpectedEof, parser.pos));
    }
    if !parser.peek_is(b'<') || parser.starts_with("<![CDATA[") {
        let text = char_data(parser)?;
        cursor.nodes.text();
        return Ok(Scanned::Event(text));
    }
    if parser.starts_with("</") {
        return end_tag(parser, &mut cursor.open);
    }
    if let Some(event) = misc(parser)? {
        cursor.nodes.add(1);
        return Ok(Scanned::Event(event));
    }
    start_tag(parser, cursor)
}

/// Consume the XML declaration, and nothing after it.
///
/// `skip_prolog` is the one way to the declaration checks, and it
/// goes on to consume any `DOCTYPE` that follows -- which may not have
/// wholly arrived. It is given the declaration alone to work on, so it
/// cannot run on past it.
fn declaration(parser: &mut Parser<'_>) -> Result<()> {
    let (input, bytes) = (parser.input, parser.bytes);
    let end = declaration_end(input);
    parser.input = &input[..end];
    parser.bytes = &bytes[..end];
    let outcome = parser.skip_prolog();
    parser.input = input;
    parser.bytes = bytes;
    outcome
}

/// Where the XML declaration ends, by the rule `skip_prolog` uses to
/// recognise one: `<?xml` not followed by a name character. Without a
/// `?>` it runs to the end, where `skip_prolog` reports it.
fn declaration_end(input: &str) -> usize {
    let lead =
        input.len() - input.trim_start_matches([' ', '\t', '\r', '\n']).len();
    let Some(rest) = input[lead..].strip_prefix("<?xml") else {
        return 0;
    };
    if rest
        .bytes()
        .next()
        .is_some_and(|b| crate::parser::is_name_char(char::from(b)))
    {
        return 0;
    }
    rest.find("?>")
        .map_or(input.len(), |i| lead + "<?xml".len() + i + "?>".len())
}

/// Reads one event from outside the root element: the
/// `prolog`/`Misc*` grammar rather than element content. Any
/// `DOCTYPE` here has already been consumed by [`eventless_step`].
fn outside_root<'a>(
    parser: &mut Parser<'a>,
    cursor: &mut Cursor,
) -> Result<Scanned<'a>> {
    parser.skip_whitespace();
    if parser.pos >= parser.bytes.len() {
        return at_end(cursor.seen_root, parser.pos);
    }
    if !parser.peek_is(b'<') {
        // Non-whitespace character data outside the root element
        // is never well-formed.
        return Err(Error::new(ErrorKind::TrailingContent, parser.pos));
    }
    if let Some(event) = misc(parser)? {
        cursor.nodes.add(1);
        return Ok(Scanned::Event(event));
    }
    if parser.starts_with("</") {
        let name = parser.peek_end_tag_name();
        return Err(Error::new(ErrorKind::UnexpectedEndTag(name), parser.pos));
    }
    if cursor.seen_root {
        return Err(Error::new(ErrorKind::TrailingContent, parser.pos));
    }
    cursor.seen_root = true;
    start_tag(parser, cursor)
}

fn at_end(seen_root: bool, pos: usize) -> Result<Scanned<'static>> {
    if seen_root {
        Ok(Scanned::Eof)
    } else {
        // A document with no element in it is not one.
        Err(Error::new(ErrorKind::NoRootElement, pos))
    }
}

/// A `DOCTYPE` met outside the root element.
///
/// Legal once and only before the root -- except that the tree parser
/// consumes every one directly after the declaration (see
/// [`Cursor::prolog_doctypes`]), and agreement with it is the
/// contract.
fn doctype(parser: &mut Parser<'_>, cursor: &Cursor) -> Result<()> {
    if !cursor.prolog_doctypes && (cursor.seen_root || parser.dtd.is_some()) {
        return Err(Error::new(ErrorKind::TrailingContent, parser.pos));
    }
    parser.skip_doctype()
}

/// A comment or processing instruction, which may appear anywhere,
/// or `None` if what comes next is neither.
fn misc<'a>(parser: &mut Parser<'a>) -> Result<Option<BorrowedEvent<'a>>> {
    if parser.starts_with("<!--") {
        let c = parser.parse_comment()?;
        return Ok(Some(BorrowedEvent::Comment(chars_to_cow(parser, c))));
    }
    if parser.starts_with("<?") {
        let (target, data) = parser.parse_pi()?;
        return Ok(Some(BorrowedEvent::ProcessingInstruction {
            target: chars_to_cow(parser, target),
            data: chars_to_cow(parser, data),
        }));
    }
    Ok(None)
}

/// Consumes one character-data run into a single `Text` event.
///
/// Text, `CDATA` sections and references in any order are all
/// character data, so they belong to one run and end at the first
/// construct that is not. The tree parser accumulates exactly this
/// run into one text node.
fn char_data<'a>(parser: &mut Parser<'a>) -> Result<BorrowedEvent<'a>> {
    let mut run = Run::default();
    loop {
        if parser.starts_with("<![CDATA[") {
            parser.parse_cdata(&mut run)?;
        } else if parser.pos < parser.bytes.len() && !parser.peek_is(b'<') {
            parser.parse_text_run(&mut run)?;
        } else {
            break;
        }
    }
    // The same limit the tree parser applies when it flushes a run
    // into a text node.
    if parser.limits.max_text_length.is_some_and(|m| run.len() > m) {
        return Err(Error::new(ErrorKind::TextTooLong, parser.pos));
    }
    Ok(BorrowedEvent::Text(run_to_cow(parser.input, &run)))
}

/// An end tag, which must close the innermost open element.
fn end_tag<'a>(
    parser: &mut Parser<'a>,
    open: &mut Vec<(String, ExpandedName)>,
) -> Result<Scanned<'a>> {
    let start = parser.pos;
    parser.pos += 2;
    let close = parser.parse_name()?;
    parser.skip_whitespace();
    if !parser.peek_is(b'>') {
        return Err(Error::new(ErrorKind::Unterminated("end tag"), start));
    }
    parser.pos += 1;
    // `open` is non-empty: an end tag at top level was reported by
    // `outside_root` before reaching here.
    let (expected, name) = open.pop().expect("inside an element");
    if close != expected {
        return Err(Error::new(
            ErrorKind::MismatchedEndTag {
                expected,
                found: close.to_owned(),
            },
            start,
        ));
    }
    parser.ns.pop_scope();
    parser.depth -= 1;
    Ok(Scanned::Event(BorrowedEvent::EndElement {
        name,
        prefix: prefix_from(expected),
    }))
}

/// The prefix of a qualified name the stack owned, reusing its
/// allocation rather than copying the part before the colon.
fn prefix_from(mut qname: String) -> Option<Cow<'static, str>> {
    let colon = qname.find(':')?;
    qname.truncate(colon);
    Some(Cow::Owned(qname))
}

fn prefix_of(qname: &str) -> Option<&str> {
    qname.split_once(':').map(|(prefix, _)| prefix)
}

/// A start tag, and the end event a self-closing one implies.
fn start_tag<'a>(
    parser: &mut Parser<'a>,
    cursor: &mut Cursor,
) -> Result<Scanned<'a>> {
    // Before scanning, not after. `parse_element` checks depth on
    // entry, so for a malformed tag at the limit the tree parser
    // reports the depth and the reader must too.
    if parser.depth >= parser.limits.max_depth {
        return Err(Error::new(ErrorKind::DepthLimitExceeded, parser.pos));
    }
    let tag = parser.scan_start_tag()?;
    let at = tag.tag_start;
    let attributes = attributes(parser, tag.raw_attrs, tag.qname, at)?;
    // The tree parser's arena holds the element, a node per attribute,
    // a node per declaration and, on the root element, one for the
    // `xml` prefix every document has.
    let is_root = parser.depth == 0;
    cursor
        .nodes
        .add(1 + attributes.len() + tag.declared.len() + usize::from(is_root));
    let name = resolve(parser, tag.qname, true, at)?;
    let start = BorrowedEvent::StartElement {
        name: name.clone(),
        prefix: prefix_of(tag.qname).map(Cow::Borrowed),
        attributes,
        namespaces: namespaces(tag.declared),
    };
    if tag.self_closing {
        parser.ns.pop_scope();
        let pending = Pending {
            name,
            prefix: prefix_of(tag.qname).map(ToOwned::to_owned),
        };
        return Ok(Scanned::SelfClosed(start, pending));
    }
    parser.depth += 1;
    cursor.open.push((tag.qname.to_owned(), name));
    Ok(Scanned::Event(start))
}

/// Namespace declarations as the event reports them: `None` for the
/// default namespace, which the scanner reports as an empty prefix.
fn namespaces(
    declared: Vec<(String, String)>,
) -> Vec<(Option<String>, String)> {
    declared
        .into_iter()
        .map(|(prefix, uri)| ((!prefix.is_empty()).then_some(prefix), uri))
        .collect()
}

/// Resolve and intern a name, as the tree parser does.
fn resolve(
    parser: &mut Parser<'_>,
    qname: &str,
    is_element: bool,
    at: usize,
) -> Result<ExpandedName> {
    let id = parser.intern_qname(qname, is_element, at)?;
    Ok(parser
        .doc
        .name(id)
        .cloned()
        .unwrap_or_else(|| ExpandedName::local(qname)))
}

/// Resolve a start tag's attributes, dropping namespace declarations
/// and refusing duplicates.
///
/// Compared by expanded name, as the tree parser does: Namespaces in
/// XML forbids two attributes with one expanded name however they are
/// spelled.
fn attributes<'a>(
    parser: &mut Parser<'a>,
    raw_attrs: Vec<(&'a str, Run)>,
    element: &str,
    at: usize,
) -> Result<Vec<(ExpandedName, Cow<'a, str>)>> {
    let mut attributes = Vec::with_capacity(raw_attrs.len());
    for (raw, value) in raw_attrs {
        if raw == "xmlns" || raw.starts_with("xmlns:") {
            continue;
        }
        let name = resolve(parser, raw, false, at)?;
        if attributes
            .iter()
            .any(|(seen, _): &(ExpandedName, _)| *seen == name)
        {
            return Err(Error::new(
                ErrorKind::DuplicateAttribute(raw.to_owned()),
                at,
            ));
        }
        // XML 1.0 section 3.3.3: a value whose declared type is not
        // `CDATA` gets a second pass, as the tree parser gives it.
        let tokenized = parser
            .dtd
            .as_ref()
            .is_some_and(|d| d.is_tokenized_attribute(element, raw));
        attributes
            .push((name, attribute_value(parser.input, &value, tokenized)));
    }
    Ok(attributes)
}

/// An attribute's value, borrowed when the input holds it verbatim.
fn attribute_value<'a>(
    input: &'a str,
    value: &Run,
    tokenized: bool,
) -> Cow<'a, str> {
    let text = value.as_str(input);
    if tokenized {
        let collapsed = collapse(text);
        if collapsed != text {
            return Cow::Owned(collapsed);
        }
        // Collapsing changed nothing, so the slice still says it.
    }
    match value.as_span() {
        Some((start, len)) => Cow::Borrowed(&input[start..start + len]),
        None => Cow::Owned(text.to_owned()),
    }
}

/// The further normalisation of a non-`CDATA` attribute value:
/// leading and trailing spaces discarded, runs collapsed to one. The
/// same rewriting as the tree parser's private `collapse`.
fn collapse(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for part in text.split(' ').filter(|p| !p.is_empty()) {
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(part);
    }
    out
}

fn run_to_cow<'a>(input: &'a str, run: &Run) -> Cow<'a, str> {
    match run.as_span() {
        Some((start, len)) => Cow::Borrowed(&input[start..start + len]),
        None => Cow::Owned(run.as_str(input).to_owned()),
    }
}

/// Convert a parser character span/expansion into a `Cow<'a, str>`.
fn chars_to_cow<'a>(parser: &Parser<'a>, c: Chars) -> Cow<'a, str> {
    match c {
        Chars::Span(s, l) => {
            let start = s as usize;
            let end = (s + l) as usize;
            Cow::Borrowed(parser.input.get(start..end).unwrap_or_default())
        }
        Chars::Expanded(_) => Cow::Owned(parser.owned(c).to_owned()),
    }
}
