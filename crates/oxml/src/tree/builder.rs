// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! A chainable front for the mutation primitives.

use super::{Document, NodeError, NodeId};

/// A chainable way to build a subtree.
///
/// The plain methods on [`Document`] are the primitives; this is a
/// convenience over them, and it does nothing they cannot.
///
/// # A note on `unused_results`
///
/// Every method returns `&mut Self` so the calls chain. Under
/// `#![deny(unused_results)]` -- which this crate itself uses -- the
/// final call in a closure needs binding:
///
/// ```text
/// .child("title", |t| { let _ = t.text("Dune"); })
/// ```
///
/// That is the cost of a chainable API in a codebase with that lint,
/// and it is worth stating rather than leaving a caller to discover it
/// from a compiler error in their own crate.
///
/// # Errors are sticky
///
/// A chain cannot return a `Result` at every step without becoming
/// unreadable, so the first error is recorded and every later call
/// becomes a no-op. [`Builder::finish`] reports it. That means a chain
/// never panics and never silently half-applies: either the whole
/// subtree is built, or `finish` tells you which step failed.
///
/// # Example
///
/// ```
/// use oxml::tree::Document;
///
/// let mut doc = Document::empty();
/// let root = doc.root();
/// let catalogue = doc
///     .build(root, "catalogue")
///     .attr("version", "1.0")
///     .child("book", |b| {
///         b.attr("year", "1965").child("title", |t| {
///             t.text("Dune");
///         });
///     })
///     .finish()
///     .expect("the root is live");
///
/// assert_eq!(doc.children(catalogue).len(), 1);
/// assert_eq!(doc.text(root), "Dune");
/// ```
#[derive(Debug)]
pub struct Builder<'a> {
    doc: &'a mut Document,
    id: NodeId,
    /// The first failure, if any. Kept rather than returned so the
    /// chain reads as one expression.
    error: Option<NodeError>,
}

impl Builder<'_> {
    /// Record the outcome of one step, keeping the first failure.
    fn step(&mut self, outcome: Result<(), NodeError>) -> &mut Self {
        if self.error.is_none() {
            self.error = outcome.err();
        }
        self
    }

    /// Set an attribute on the element being built.
    pub fn attr(&mut self, local: &str, value: &str) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        let outcome = self.doc.set_attribute(self.id, None, local, value);
        self.step(outcome)
    }

    /// Set a namespaced attribute.
    pub fn attr_ns(
        &mut self,
        namespace: &str,
        local: &str,
        value: &str,
    ) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        let outcome =
            self.doc
                .set_attribute(self.id, Some(namespace), local, value);
        self.step(outcome)
    }

    /// Append text to the element being built.
    pub fn text(&mut self, text: &str) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        let outcome = self.doc.append_text(self.id, text).map(|_| ());
        self.step(outcome)
    }

    /// Append a child element and build it with `f`.
    ///
    /// The closure receives a builder for the child. Only one mutable
    /// borrow of the document exists at a time -- the child's builder
    /// reborrows this one's -- which is why the closure takes a
    /// `&mut Builder` rather than returning one.
    pub fn child(
        &mut self,
        local: &str,
        f: impl FnOnce(&mut Builder<'_>),
    ) -> &mut Self {
        self.child_in(None, local, f)
    }

    /// Append a child element in a namespace and build it with `f`.
    ///
    /// `namespace` is the namespace name (the URI); the prefix is
    /// chosen when the document is written, as for
    /// [`Document::append_element`].
    pub fn child_ns(
        &mut self,
        namespace: &str,
        local: &str,
        f: impl FnOnce(&mut Builder<'_>),
    ) -> &mut Self {
        self.child_in(Some(namespace), local, f)
    }

    fn child_in(
        &mut self,
        namespace: Option<&str>,
        local: &str,
        f: impl FnOnce(&mut Builder<'_>),
    ) -> &mut Self {
        if self.error.is_some() {
            return self;
        }
        match self.doc.append_element(self.id, namespace, local) {
            Ok(child) => {
                let mut inner = Builder {
                    doc: &mut *self.doc,
                    id: child,
                    error: None,
                };
                f(&mut inner);
                // A failure inside the closure is a failure of the
                // whole chain; swallowing it would report success over
                // a subtree that is not there.
                self.error = inner.error;
            }
            Err(e) => self.error = Some(e),
        }
        self
    }

    /// The element that was built, or the first error along the way.
    ///
    /// # Errors
    ///
    /// Whatever the first failing step returned.
    pub fn finish(&mut self) -> Result<NodeId, NodeError> {
        match self.error {
            Some(e) => Err(e),
            None => Ok(self.id),
        }
    }
}

impl Document {
    /// Append an element to `parent` and return a builder for it.
    ///
    /// # Errors
    ///
    /// Reported by [`Builder::finish`] rather than here, so the chain
    /// reads as one expression. If `parent` is stale, cannot hold
    /// children, or already has a root element, `finish` returns that
    /// error and nothing was built.
    pub fn build<'a>(&'a mut self, parent: NodeId, local: &str) -> Builder<'a> {
        match self.append_element(parent, None, local) {
            Ok(id) => Builder {
                doc: self,
                id,
                error: None,
            },
            Err(e) => Builder {
                doc: self,
                // Any identifier will do: every method checks `error`
                // first and does nothing, and `finish` returns the
                // error rather than this.
                id: parent,
                error: Some(e),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nested_subtree_is_built_in_one_expression() {
        let mut doc = Document::empty();
        let root = doc.root();
        let cat = doc
            .build(root, "catalogue")
            .attr("version", "1.0")
            .child("book", |b| {
                let _ = b.attr("year", "1965").child("title", |t| {
                    let _ = t.text("Dune");
                });
            })
            .child("book", |b| {
                let _ = b.attr("year", "1961").child("title", |t| {
                    let _ = t.text("Solaris");
                });
            })
            .finish()
            .expect("root is live");

        assert_eq!(doc.children(cat).len(), 2);
        assert_eq!(doc.attribute(cat, "version"), Some("1.0"));
        assert_eq!(doc.text(root), "DuneSolaris");
    }

    #[test]
    fn the_builder_produces_the_same_tree_as_the_primitives() {
        // The builder must be a convenience over the primitives and
        // nothing more. If the two disagree, one of them is wrong.
        let mut built = Document::empty();
        let root = built.root();
        let _ = built
            .build(root, "a")
            .attr("k", "v")
            .child("b", |b| {
                let _ = b.text("x");
            })
            .child_ns("urn:n", "c", |c| {
                let _ = c.attr_ns("urn:n", "k", "v");
            })
            .finish()
            .expect("live");

        let mut manual = Document::empty();
        let mroot = manual.root();
        let a = manual.append_element(mroot, None, "a").expect("live");
        manual.set_attribute(a, None, "k", "v").expect("live");
        let b = manual.append_element(a, None, "b").expect("live");
        let _ = manual.append_text(b, "x").expect("live");
        let c = manual.append_element(a, Some("urn:n"), "c").expect("live");
        manual
            .set_attribute(c, Some("urn:n"), "k", "v")
            .expect("live");

        assert_eq!(built.to_xml(), manual.to_xml());
    }

    #[test]
    fn the_first_error_is_reported_and_later_steps_do_nothing() {
        // A chain cannot return a Result at every step. The contract
        // is that it never half-applies silently: either the subtree
        // is built or `finish` says which step failed.
        let mut doc = Document::empty();
        let root = doc.root();
        let _first = doc.append_element(root, None, "one").expect("live");

        let before = doc.len();
        let result = doc
            .build(root, "two")
            .attr("k", "v")
            .child("c", |b| {
                let _ = b.text("x");
            })
            .finish();

        assert_eq!(result, Err(NodeError::RootElementExists));
        assert_eq!(doc.len(), before, "nothing was added after the failure");
    }

    #[test]
    fn a_failure_inside_a_closure_fails_the_whole_chain() {
        // Validation can now fail inside a closure, so the propagation
        // in `child_in` is exercised rather than merely defensive.
        let mut doc = Document::empty();
        let root = doc.root();
        let before = doc.len();
        let result = doc
            .build(root, "a")
            .child("b", |b| {
                let _ = b.child("not a name", |_| {}).text("after");
            })
            .finish();
        assert_eq!(result, Err(NodeError::InvalidName));
        assert_eq!(doc.len(), before + 2, "a and b were built, nothing after");
    }

    #[test]
    fn building_on_a_stale_parent_reports_it_rather_than_panicking() {
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        let b = doc.append_element(a, None, "b").expect("live");
        doc.remove(a).expect("live");

        let result = doc.build(b, "c").attr("k", "v").finish();
        assert_eq!(result, Err(NodeError::Stale));
    }

    #[test]
    fn building_under_a_text_node_is_refused() {
        let mut doc = Document::empty();
        let root = doc.root();
        let a = doc.append_element(root, None, "a").expect("live");
        let t = doc.append_text(a, "t").expect("live");
        assert_eq!(doc.build(t, "c").finish(), Err(NodeError::NotAContainer));
        assert!(doc.children(t).is_empty());
    }

    #[test]
    fn a_built_document_round_trips() {
        let mut doc = Document::empty();
        let root = doc.root();
        let _ = doc
            .build(root, "r")
            .attr("note", "a & b < c")
            .child("child", |c| {
                let _ = c.text("text & more");
            })
            .finish()
            .expect("live");

        let xml = doc.to_xml();
        let reparsed = crate::parse(&xml).expect("must parse");
        assert_eq!(reparsed.to_xml(), xml);
        assert_eq!(reparsed.text(reparsed.root()), "text & more");
    }
}
