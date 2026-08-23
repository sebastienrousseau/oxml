// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! # oxml
//!
//! A pure Rust XML toolkit. Zero unsafe code. Parsing, an ergonomic
//! tree, and `XPath` 1.0.
//!
//! ## Why this exists
//!
//! Rust's XML ecosystem is strong at one end and empty at the other.
//! `quick-xml` and `roxmltree` parse quickly; nothing maintained
//! offers what `lxml` gives Python. The only `XPath` crate,
//! `sxd-xpath`, has not shipped a release since 2018, and XSLT and
//! XSD validation have no pure-Rust implementation at all.
//!
//! oxml closes the query gap first, because that is the one people
//! actually hit.
//!
//! ## Quick Start
//!
//! ```
//! # #[cfg(all(feature = "xpath", feature = "std"))] {
//! use oxml::{parse, XPath};
//!
//! let doc = parse(r#"
//!     <library>
//!         <book lang="en"><title>Dune</title></book>
//!         <book lang="fr"><title>Germinal</title></book>
//!     </library>
//! "#).unwrap();
//!
//! let titles = XPath::compile("//book[@lang='en']/title").unwrap();
//! let found = titles.evaluate(&doc);
//!
//! assert_eq!(found.to_str(&doc), "Dune");
//! # }
//! ```
//!
//! ## Walking the tree directly
//!
//! `XPath` is optional. The tree stands on its own:
//!
//! ```
//! use oxml::parse;
//!
//! let doc = parse("<a><b id='1'>text</b></a>")?;
//! let root = doc.root_element().expect("a root element");
//!
//! assert_eq!(doc.element_name(root).unwrap().local, "a");
//!
//! let b = doc.children(root)[0];
//! assert_eq!(doc.attribute(b, "id"), Some("1"));
//! assert_eq!(doc.text(b), "text");
//! # Ok::<(), oxml::Error>(())
//! ```
//!
//! ## Design
//!
//! - **Zero `unsafe`** — `#![forbid(unsafe_code)]`, enforced at
//!   compile time. The tree is an arena of index-addressed nodes, so
//!   parent links cost no `Rc`, no `RefCell`, and no raw pointers.
//!
//! - **No entity expansion** — only the five predefined entities and
//!   numeric character references are resolved. External and custom
//!   entities are not, which forecloses XXE and billion-laughs by
//!   construction rather than by configuration. A parser that cannot
//!   expand them cannot be talked into leaking a file.
//!
//! - **Namespace-correct** — names compare by URI and local part,
//!   never by prefix. An unprefixed *element* takes the default
//!   namespace; an unprefixed *attribute* is in no namespace. That
//!   asymmetry is the classic source of namespace bugs, so it is
//!   explicit in the parser rather than assumed.
//!
//! ## Feature flags
//!
//! - `std` *(default)* — standard library integration, including
//!   `std::error::Error`.
//! - `xpath` *(default)* — the `XPath` engine. Turn it off if you only
//!   need to parse.

#![forbid(unsafe_code)]
#![warn(missing_docs)]
#![cfg_attr(not(feature = "std"), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;

#[cfg(feature = "std")]
extern crate std;

pub mod error;
mod parser;
pub mod tree;

#[cfg(feature = "xpath")]
#[cfg_attr(docsrs, doc(cfg(feature = "xpath")))]
pub mod xpath;

/// The README's examples, compiled as doctests.
///
/// `include_str!` rather than a copy: a snapshot of the README in a
/// test file drifts from the README the moment either is edited, and a
/// check that silently stops checking is worse than no check. This way
/// every ```rust block in the README is compiled and run by
/// `cargo test`, and a broken example fails the build.
/// Gated on the features the README demonstrates: its examples use
/// XPath, so under `--no-default-features` they would fail to compile
/// for a reason that says nothing about the crate.
#[cfg(all(doctest, feature = "xpath", feature = "std"))]
#[doc = include_str!("../README.md")]
pub struct ReadmeDoctests;

/// The deepest element nesting the parser will accept.
///
/// Parsing descends one stack frame per open element, so an
/// arbitrarily deep document would exhaust the stack — and a stack
/// overflow aborts the process rather than unwinding, so no caller can
/// catch it. Documents come from the network in every one of this
/// crate's front ends, which makes that a denial of service rather
/// than a curiosity.
///
/// The limit is well above any hand-written document and far below the
/// depth that threatens the smallest stack a caller is likely to have
/// (test harnesses commonly give threads 2 MiB).
pub const MAX_DEPTH: usize = 256;

mod dtd;
pub mod encoding;
mod limits;
mod names4e;
pub use limits::{Edition, Limits};

pub use error::{Error, ErrorKind, Result};
pub use parser::{parse, parse_bytes, parse_bytes_with, parse_with};
pub use tree::{Attribute, Document, ExpandedName, NodeId, NodeKind};

#[cfg(feature = "xpath")]
pub use xpath::{XPath, XPathError};
