// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Axes and node tests: the two halves of a step before its
//! predicates, with prefix resolution for name tests.

use alloc::borrow::ToOwned;
use alloc::string::String;

use super::{P, XPathError};
use crate::xpath::ast::{Axis, NodeTest};

impl P<'_> {
    /// `..` or `.`, checked in that order: `..` would otherwise lex as
    /// two `.` steps.
    pub(super) fn eat_abbreviated_step(&mut self) -> Option<Axis> {
        if self.s[self.i..].starts_with("..") {
            self.i += 2;
            return Some(Axis::Parent);
        }
        if self.peek() == Some(b'.') {
            self.i += 1;
            return Some(Axis::SelfAxis);
        }
        None
    }

    /// `@`, an explicit `axis::`, or the child axis by default.
    pub(super) fn parse_axis(&mut self) -> Result<Axis, XPathError> {
        if self.eat("@") {
            return Ok(Axis::Attribute);
        }
        let save = self.i;
        match self.try_name() {
            Some(n) if self.s[self.i..].starts_with("::") => {
                self.i += 2;
                Axis::from_name(&n).ok_or_else(|| self.err("unknown axis"))
            }
            _ => {
                self.i = save;
                Ok(Axis::Child)
            }
        }
    }

    pub(super) fn parse_node_test(&mut self) -> Result<NodeTest, XPathError> {
        self.ws();
        if self.eat("*") {
            return self.parse_wildcard_test();
        }
        let name = self
            .try_name()
            .ok_or_else(|| self.err("expected a node test"))?;
        self.ws();
        if self.peek() == Some(b'(') {
            self.i += 1;
            return self.parse_kind_test(&name);
        }
        self.resolve_name_test(name)
    }

    /// After `*`: the wildcard, or `*:local` -- any namespace, one
    /// local name. An extension (`XPath` 2.0's production, not in 1.0),
    /// accepted only with the colon immediately adjacent, as 2.0's
    /// lexical rule requires. In node-test position `*` is never
    /// multiplication, so there is no ambiguity to resolve.
    pub(super) fn parse_wildcard_test(
        &mut self,
    ) -> Result<NodeTest, XPathError> {
        if self.peek() != Some(b':') {
            return Ok(NodeTest::Wildcard);
        }
        self.i += 1;
        let local = self
            .try_name()
            .ok_or_else(|| self.err("expected a name after `*:`"))?;
        // `try_name` reads a full QName; a prefix here would make the
        // expression `*:p:l`, which nothing defines.
        if local.contains(':') {
            return Err(
                self.err("`*:` takes a local name, not a qualified one")
            );
        }
        Ok(NodeTest::AnyNamespace { local })
    }

    /// A node-type test, after its opening parenthesis.
    /// `processing-instruction` is the one that takes an argument: an
    /// optional literal naming the target.
    pub(super) fn parse_kind_test(
        &mut self,
        name: &str,
    ) -> Result<NodeTest, XPathError> {
        if name == "processing-instruction" {
            self.ws();
            let target = self.try_literal();
            self.ws();
            if !self.eat(")") {
                return Err(self.err("expected )"));
            }
            return Ok(NodeTest::ProcessingInstruction(target));
        }
        if !self.eat(")") {
            return Err(self.err("expected ()"));
        }
        match name {
            "text" => Ok(NodeTest::Text),
            "comment" => Ok(NodeTest::Comment),
            "node" => Ok(NodeTest::Any),
            _ => Err(self.err("unknown node type")),
        }
    }

    /// A name test, with its prefix resolved to a URI. Unprefixed, it
    /// matches nodes in no namespace only.
    pub(super) fn resolve_name_test(
        &self,
        name: String,
    ) -> Result<NodeTest, XPathError> {
        match name.split_once(':') {
            Some((prefix, local)) => Ok(NodeTest::Name {
                namespace: Some(self.resolve_prefix(prefix)?.to_owned()),
                local: local.to_owned(),
            }),
            None => Ok(NodeTest::Name {
                namespace: None,
                local: name,
            }),
        }
    }

    /// The URI a prefix is bound to. `xml` is bound by specification
    /// and never declared.
    pub(super) fn resolve_prefix(
        &self,
        prefix: &str,
    ) -> Result<&str, XPathError> {
        if prefix == "xml" {
            return Ok("http://www.w3.org/XML/1998/namespace");
        }
        self.namespaces
            .iter()
            .find(|(p, _)| *p == prefix)
            .map(|(_, uri)| *uri)
            .ok_or_else(|| {
                self.err(&alloc::format!(
                    "unbound namespace prefix `{prefix}`; bind it with \
                     XPath::compile_with_namespaces"
                ))
            })
    }
}
