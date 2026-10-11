// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Whether the construct at a position has wholly arrived.
//!
//! The scanner is never handed a partial construct. Speculatively
//! scanning and retrying on failure would be simpler to write and
//! wrong: a half-scanned start tag has already interned a name and
//! pushed a namespace scope, so the retry would see state the first
//! attempt left behind. Instead the end of the construct is found
//! first, by delimiters alone -- `-->`, `]]>`, `?>`, `>` -- which is
//! cheap and needs no understanding of what lies between them.
//!
//! The search resumes where it stopped. A pull adds a few kilobytes,
//! and searching from the construct's start after each one made a
//! long text node quadratic in its length: 16 MB took 835 ms.

/// Which construct begins at a position, decided from its opener.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    /// Character data: text, `CDATA` sections and references, which
    /// run on until a `<` that opens something else.
    Text,
    /// `<!--` … `-->`.
    Comment,
    /// `<?` … `?>`.
    Pi,
    /// `<!DOCTYPE` … `>`, with everything an internal subset may hold.
    Doctype,
    /// A start or end tag, to the first `>` outside a quoted value.
    Tag,
}

/// Openers that decide a kind, longest first where one is a prefix
/// of another's beginning.
const OPENERS: [(&[u8], Kind); 4] = [
    (b"<!--", Kind::Comment),
    (b"<![CDATA[", Kind::Text),
    (b"<?", Kind::Pi),
    (b"<!DOCTYPE", Kind::Doctype),
];

const CDATA_OPEN: &[u8] = b"<![CDATA[";

impl Kind {
    /// Bytes of the opener, past which the closer may begin.
    ///
    /// `<!-->` is not a comment: the `-->` must follow the opener, not
    /// overlap it. A text run has no opener: a `CDATA` section at its
    /// start is handled by the run's own state.
    const fn opener_len(self) -> usize {
        match self {
            Self::Text => 0,
            Self::Tag => 1,
            Self::Pi => 2,
            Self::Comment => 4,
            Self::Doctype => 9,
        }
    }

    /// The kind of the construct beginning `rest`, or `None` while
    /// too little of it has arrived to say -- `<!` may yet become a
    /// comment, a `CDATA` section or a doctype.
    fn classify(rest: &[u8]) -> Option<Self> {
        if rest.is_empty() {
            return None;
        }
        if rest[0] != b'<' {
            return Some(Self::Text);
        }
        for (opener, kind) in OPENERS {
            if rest.starts_with(opener) {
                return Some(kind);
            }
            if opener.starts_with(rest) {
                return None;
            }
        }
        Some(Self::Tag)
    }
}

/// Where a text run's search is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextState {
    /// Between markup, looking for the next `<`.
    Chars,
    /// Inside a `CDATA` section, looking for `]]>`.
    Cdata,
}

/// Where a doctype's search is.
///
/// A doctype is the one construct whose end cannot be found by
/// searching for a delimiter: `>` is ordinary inside a comment, a
/// processing instruction, a quoted literal or a declaration of the
/// internal subset, and `]` closes the subset only when nothing is
/// open inside it. Counting brackets alone ended
/// `<!DOCTYPE a [<!-- ] --> …` at the comment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DtdState {
    /// After the name: an external id, then `[` or `>`.
    Head,
    /// Inside a system or public literal in the head.
    HeadQuote(u8),
    /// Between declarations of the internal subset.
    Subset,
    /// Inside `<!--` … `-->` in the subset.
    Comment,
    /// Inside `<?` … `?>` in the subset.
    Pi,
    /// Inside a markup declaration, to its `>`.
    Decl,
    /// Inside a quoted literal of a declaration.
    DeclQuote(u8),
    /// Inside `<![` … `]]>`, with the nesting depth.
    Conditional(usize),
    /// After the `]` that closed the subset: whitespace, then `>`.
    Tail,
}

/// What one step of the doctype machine decided.
enum Step {
    /// Consumed this many bytes; keep going.
    Advance(usize),
    /// The doctype ends here.
    Whole,
    /// The bytes in hand could be the start of a longer token.
    NeedMore,
}

/// The search for the end of one construct, resumable across pulls.
#[derive(Debug, Clone, Copy)]
pub(super) struct Probe {
    /// Where the construct begins.
    pub(super) start: usize,
    /// Decided once enough of the opener has arrived.
    kind: Option<Kind>,
    /// Bytes before this have been searched and hold no closer.
    scanned_to: usize,
    /// The search may not resume before this, whatever the closer's
    /// overlap: the opener's end, or a `CDATA` body's start.
    floor: usize,
    /// Inside a quoted attribute value, the quote that opened it.
    quote: Option<u8>,
    text: TextState,
    dtd: DtdState,
}

impl Probe {
    pub(super) const fn at(start: usize) -> Self {
        Self {
            start,
            kind: None,
            scanned_to: start,
            floor: start,
            quote: None,
            text: TextState::Chars,
            dtd: DtdState::Head,
        }
    }

    /// Move every position back by `by`, after that many bytes were
    /// dropped off the front of the buffer.
    pub(super) fn shift(&mut self, by: usize) {
        self.start -= by;
        self.scanned_to -= by;
        self.floor -= by;
    }

    /// Whether the construct ends within `text`.
    ///
    /// Outside the root element character data is whole at once: the
    /// scanner refuses it on sight, and the refusal does not depend on
    /// how much of it there is.
    pub(super) fn is_whole(&mut self, text: &str, in_content: bool) -> bool {
        let Some(kind) = self.decide(text) else {
            return false;
        };
        match kind {
            Kind::Text if !in_content => true,
            Kind::Text => self.text_end(text),
            Kind::Comment => self.find(text, "-->"),
            Kind::Pi => self.find(text, "?>"),
            Kind::Doctype => self.doctype_end(text),
            Kind::Tag => self.tag_end(text),
        }
    }

    /// The construct's kind, decided on first sight of enough of its
    /// opener and remembered after.
    fn decide(&mut self, text: &str) -> Option<Kind> {
        if self.kind.is_none() {
            let kind = Kind::classify(&text.as_bytes()[self.start..])?;
            self.kind = Some(kind);
            self.floor = self.start + kind.opener_len();
            self.scanned_to = self.floor;
        }
        self.kind
    }

    /// Search for `closer` from where the last search stopped.
    ///
    /// Resumes `closer.len() - 1` bytes back, since the closer may
    /// straddle the previous end of the buffer, but never before the
    /// floor, and always on a character boundary. On success the
    /// position is left just past the closer.
    fn find(&mut self, text: &str, closer: &str) -> bool {
        let mut from = self
            .scanned_to
            .saturating_sub(closer.len() - 1)
            .max(self.floor);
        while !text.is_char_boundary(from) {
            from -= 1;
        }
        if let Some(i) = text[from..].find(closer) {
            self.scanned_to = from + i + closer.len();
            return true;
        }
        self.scanned_to = text.len();
        false
    }

    /// A text run ends at the first `<` that does not open a `CDATA`
    /// section. Text, sections and references in any order are one
    /// run, because the tree parser accumulates exactly that into one
    /// text node; ending the run at a section split `one <![CDATA[two]]>
    /// three` into three events through a small buffer and one through
    /// a large one.
    fn text_end(&mut self, text: &str) -> bool {
        loop {
            match self.text {
                TextState::Cdata => {
                    if !self.find(text, "]]>") {
                        return false;
                    }
                    self.text = TextState::Chars;
                    self.floor = self.scanned_to;
                }
                TextState::Chars => match self.next_markup(text) {
                    Some(true) => return true,
                    Some(false) => {
                        self.text = TextState::Cdata;
                        self.floor = self.scanned_to;
                    }
                    None => return false,
                },
            }
        }
    }

    /// Find the next `<` in a text run: `Some(true)` if it ends the
    /// run, `Some(false)` if it opens a `CDATA` section, `None` if
    /// there is none yet or too little follows it to say.
    fn next_markup(&mut self, text: &str) -> Option<bool> {
        let bytes = text.as_bytes();
        let Some(i) = bytes[self.scanned_to..].iter().position(|&b| b == b'<')
        else {
            self.scanned_to = bytes.len();
            return None;
        };
        let at = self.scanned_to + i;
        let rest = &bytes[at..];
        if rest.starts_with(CDATA_OPEN) {
            self.scanned_to = at + CDATA_OPEN.len();
            return Some(false);
        }
        self.scanned_to = at;
        if CDATA_OPEN.starts_with(rest) {
            return None;
        }
        Some(true)
    }

    /// `>` inside an attribute value is data, so quotes are tracked
    /// rather than searching for the first one.
    fn tag_end(&mut self, text: &str) -> bool {
        for (i, &b) in text.as_bytes()[self.scanned_to..].iter().enumerate() {
            match (self.quote, b) {
                (Some(q), c) if c == q => self.quote = None,
                (None, b'"' | b'\'') => self.quote = Some(b),
                (None, b'>') => {
                    self.scanned_to += i + 1;
                    return true;
                }
                _ => {}
            }
        }
        self.scanned_to = text.len();
        false
    }

    /// Drive the doctype machine over what has not been looked at.
    fn doctype_end(&mut self, text: &str) -> bool {
        let bytes = text.as_bytes();
        let mut at = self.scanned_to;
        while at < bytes.len() {
            match self.dtd_step(&bytes[at..]) {
                Step::Advance(n) => at += n,
                Step::Whole => {
                    self.scanned_to = at + 1;
                    return true;
                }
                Step::NeedMore => break,
            }
        }
        self.scanned_to = at;
        false
    }

    /// One step from the current doctype state. `rest` is never empty.
    fn dtd_step(&mut self, rest: &[u8]) -> Step {
        match self.dtd {
            DtdState::Head => self.dtd_head(rest[0]),
            DtdState::HeadQuote(q) => {
                self.dtd_quote(rest[0], q, DtdState::Head)
            }
            DtdState::Subset => self.dtd_subset(rest),
            DtdState::Comment => self.dtd_until(rest, b"-->"),
            DtdState::Pi => self.dtd_until(rest, b"?>"),
            DtdState::Decl => self.dtd_decl(rest[0]),
            DtdState::DeclQuote(q) => {
                self.dtd_quote(rest[0], q, DtdState::Decl)
            }
            DtdState::Conditional(depth) => self.dtd_conditional(rest, depth),
            DtdState::Tail => Self::dtd_tail(rest[0]),
        }
    }

    /// The head: `<!DOCTYPE name ExternalID?`, then `[` or `>`.
    fn dtd_head(&mut self, b: u8) -> Step {
        match b {
            b'"' | b'\'' => self.dtd = DtdState::HeadQuote(b),
            b'[' => self.dtd = DtdState::Subset,
            b'>' => return Step::Whole,
            _ => {}
        }
        Step::Advance(1)
    }

    /// Inside a quoted literal, everything is data until its quote.
    fn dtd_quote(&mut self, b: u8, quote: u8, back_to: DtdState) -> Step {
        if b == quote {
            self.dtd = back_to;
        }
        Step::Advance(1)
    }

    /// Between declarations: what `<` opens, or the `]` that ends
    /// the subset.
    fn dtd_subset(&mut self, rest: &[u8]) -> Step {
        const OPENERS: [(&[u8], DtdState); 4] = [
            (b"<!--", DtdState::Comment),
            (b"<![", DtdState::Conditional(1)),
            (b"<?", DtdState::Pi),
            (b"<!", DtdState::Decl),
        ];
        match rest[0] {
            b']' => {
                self.dtd = DtdState::Tail;
                Step::Advance(1)
            }
            b'<' => {
                for (opener, state) in OPENERS {
                    if rest.starts_with(opener) {
                        self.dtd = state;
                        return Step::Advance(opener.len());
                    }
                    if opener.starts_with(rest) {
                        return Step::NeedMore;
                    }
                }
                // Not a declaration at all; the DTD parser will say so.
                // Treated as one so its `>` is still waited for.
                self.dtd = DtdState::Decl;
                Step::Advance(1)
            }
            _ => Step::Advance(1),
        }
    }

    /// A comment or processing instruction in the subset: skip to its
    /// closer, keeping enough of the tail for a straddling one.
    fn dtd_until(&mut self, rest: &[u8], closer: &[u8]) -> Step {
        if let Some(i) = rest.windows(closer.len()).position(|w| w == closer) {
            self.dtd = DtdState::Subset;
            return Step::Advance(i + closer.len());
        }
        if rest.len() < closer.len() {
            return Step::NeedMore;
        }
        Step::Advance(rest.len() - closer.len() + 1)
    }

    /// Inside a markup declaration: quotes, then its `>`.
    fn dtd_decl(&mut self, b: u8) -> Step {
        match b {
            b'"' | b'\'' => self.dtd = DtdState::DeclQuote(b),
            b'>' => self.dtd = DtdState::Subset,
            _ => {}
        }
        Step::Advance(1)
    }

    /// A conditional section: `<![` and `]]>` must balance, as the DTD
    /// parser requires of an ignored section's contents.
    fn dtd_conditional(&mut self, rest: &[u8], depth: usize) -> Step {
        if rest.starts_with(b"<![") {
            self.dtd = DtdState::Conditional(depth + 1);
            return Step::Advance(3);
        }
        if rest.starts_with(b"]]>") {
            self.dtd = match depth {
                1 => DtdState::Subset,
                deeper => DtdState::Conditional(deeper - 1),
            };
            return Step::Advance(3);
        }
        if b"<![".starts_with(rest) || b"]]>".starts_with(rest) {
            return Step::NeedMore;
        }
        Step::Advance(1)
    }

    /// After the subset: whitespace, then the `>`. Anything else is
    /// malformed, and the DTD parser's verdict does not depend on
    /// what follows it.
    fn dtd_tail(b: u8) -> Step {
        if matches!(b, b' ' | b'\t' | b'\r' | b'\n') {
            return Step::Advance(1);
        }
        Step::Whole
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Where `is_whole` first says yes when `doc` arrives a byte at a
    /// time, and whether that is where the construct really ends.
    fn whole_at(doc: &str) -> Option<usize> {
        let mut probe = Probe::at(0);
        (1..=doc.len())
            .filter(|&n| doc.is_char_boundary(n))
            .find(|&n| probe.is_whole(&doc[..n], true))
    }

    #[test]
    fn a_doctype_ends_at_its_own_close_angle_bracket() {
        for (doc, end) in [
            ("<!DOCTYPE a>", 12),
            ("<!DOCTYPE a [<!-- ] --> <!ENTITY e \"x\">]>", 41),
            ("<!DOCTYPE a SYSTEM \"x>y.dtd\">", 29),
            ("<!DOCTYPE a [<!ENTITY e \"x>y\">]>", 32),
            ("<!DOCTYPE a [<?pi ] > ?><!ENTITY e 'a]b'>]>", 43),
            ("<!DOCTYPE a [<![INCLUDE[<![IGNORE[ ]]> ]]>]>", 44),
            ("<!DOCTYPE a PUBLIC \"p]\" 's>' [ ] >", 34),
            ("<!DOCTYPE a [<!ELEMENT a (#PCDATA)>\n]\n>", 39),
        ] {
            assert_eq!(whole_at(doc), Some(end), "{doc:?}");
        }
    }

    #[test]
    fn a_text_run_spans_cdata_sections_and_references() {
        let doc = "one <![CDATA[two]]> &amp; <![CDATA[]]> three<b/>";
        let lt = doc.rfind("<b/>").expect("tag");
        // One byte past the `<`: alone it could still open a section.
        assert_eq!(whole_at(doc), Some(lt + 2), "ends at the tag's `<`");
        assert_eq!(whole_at("text<"), None, "`<` alone may open CDATA");
        assert_eq!(whole_at("text<![CD"), None, "still undecided");
        assert_eq!(whole_at("text<![CDATA[x]]>"), None, "the run goes on");
        assert_eq!(whole_at("text<x"), Some(6));
    }

    #[test]
    fn delimiters_inside_quoted_attribute_values_are_data() {
        assert_eq!(whole_at("<a x='>' y=\">\">"), Some(15));
        assert_eq!(whole_at("<!-- > -->"), Some(10));
        assert_eq!(whole_at("<?pi > ?>"), Some(9));
    }

    #[test]
    fn outside_the_root_character_data_is_whole_at_once() {
        let mut probe = Probe::at(0);
        assert!(probe.is_whole("junk", false));
        assert!(!Probe::at(0).is_whole("junk", true));
    }

    #[test]
    fn the_search_resumes_rather_than_restarting() {
        let mut probe = Probe::at(0);
        let doc = format!("<!--{}-->", "x".repeat(100));
        for n in 4..doc.len() {
            assert!(!probe.is_whole(&doc[..n], true));
            assert!(probe.scanned_to >= n - 2, "resumes near the end");
        }
        assert!(probe.is_whole(&doc, true));
    }
}
