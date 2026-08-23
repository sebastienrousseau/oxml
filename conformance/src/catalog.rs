// SPDX-License-Identifier: MIT OR Apache-2.0
// Copyright (c) 2026 oxml. All rights reserved.

//! Reading the suite's manifests.

use std::path::{Path, PathBuf};

use oxml::Limits;

/// What the suite says a conforming parser must do with a document.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TestType {
    /// Every parser must accept it.
    Valid,
    /// A non-validating parser may accept it; a validating parser must
    /// reject it. `oxml` does not validate, so these are expected to
    /// parse.
    Invalid,
    /// No parser may accept it.
    NotWellFormed,
    /// Reporting is optional per the DTD, so these are not scored.
    Error,
}

impl TestType {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "valid" => Self::Valid,
            "invalid" => Self::Invalid,
            "not-wf" => Self::NotWellFormed,
            "error" => Self::Error,
            _ => return None,
        })
    }
}

/// One test case from a manifest.
#[derive(Debug, Clone)]
pub struct TestCase {
    /// The `ID` attribute, unique across the suite.
    pub id: String,
    /// What a conforming parser must do.
    pub kind: TestType,
    /// Absolute path to the document under test.
    pub path: PathBuf,
    /// Which submission it came from, e.g. `ibm`, `oasis`.
    pub submission: String,
    /// The spec revision it targets, e.g. `XML1.1`. Absent means
    /// XML 1.0.
    pub recommendation: Option<String>,
    /// `VERSION`, when the test pins one.
    pub version: Option<String>,
    /// `EDITION`, a space-separated list of 1.0 editions this applies
    /// to.
    pub edition: Option<String>,
    /// `NAMESPACE="no"` marks tests that use colons in ways the
    /// Namespaces spec forbids; a namespace-aware parser is expected to
    /// disable namespace processing for them.
    pub namespace: bool,
    /// `ENTITIES`, describing which entity kinds the document needs.
    pub entities: Option<String>,
}

/// Every test in the suite, read from the per-collection manifests.
///
/// # Errors
///
/// Returns a description if the suite directory cannot be walked or a
/// manifest cannot be read.
pub fn load(root: &Path) -> Result<Vec<TestCase>, String> {
    let mut manifests = Vec::new();
    collect_manifests(root, &mut manifests)?;
    manifests.sort();

    let mut out = Vec::new();
    for m in &manifests {
        // The top-level catalogue only includes the others by external
        // entity reference; it holds no tests of its own.
        if m.file_name().is_some_and(|n| n == "xmlconf.xml") {
            continue;
        }
        out.extend(load_manifest(root, m)?);
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(out)
}

fn collect_manifests(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), String> {
    let entries = std::fs::read_dir(dir)
        .map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.is_dir() {
            collect_manifests(&path, out)?;
        } else if path.extension().is_some_and(|e| e == "xml") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            // Detect on `<TEST `, not `<TESTCASES`. Sun's four
            // manifests have no wrapper element at all — they are bare
            // sequences of `<TEST>` designed to be pulled into
            // `xmlconf.xml`'s `<TESTCASES>` by external entity
            // reference, and so are not well-formed documents on their
            // own. Keying on the wrapper silently drops all 159 of
            // them, which is how this loader first reported 2,426 tests
            // instead of 2,585.
            if text.contains("<TEST ") {
                out.push(path);
            }
        }
    }
    Ok(())
}

fn load_manifest(
    root: &Path,
    manifest: &Path,
) -> Result<Vec<TestCase>, String> {
    let text = std::fs::read_to_string(manifest)
        .map_err(|e| format!("cannot read {}: {e}", manifest.display()))?;

    // Read the manifest with `oxml` itself. That is not circular in any
    // dangerous way — these files are plain, well-formed XML 1.0, and if
    // the parser cannot read them the conformance run fails loudly
    // rather than silently scoring zero.
    //
    // A comment can contain `<TEST`: `ibm/xml-1.1/ibm_not-wf.xml` has
    // exactly one, which is the whole difference between the 2,586 and
    // 2,585 figures both quoted in the wild. Parsing rather than
    // pattern-matching gets that right for free.
    //
    // Manifests also mix quote styles — OASIS uses single quotes
    // throughout — which is another thing a regex gets wrong and a
    // parser does not.
    // Give a fragment the root element that entity inclusion would
    // have given it. Cheap, and it means one code path reads both
    // shapes.
    let wrapped;
    let text = if text.contains("<TESTCASES") {
        text.as_str()
    } else {
        wrapped = format!("<TESTCASES>{}</TESTCASES>", strip_prolog(&text));
        wrapped.as_str()
    };

    let mut limits = Limits::permissive();
    limits.max_attributes_per_element = 100_000;
    let doc = oxml::parse_with(text, limits).map_err(|e| {
        let (line, col) = e.line_column(text);
        format!("{}:{line}:{col}: {e}", manifest.display())
    })?;

    let base = manifest.parent().unwrap_or(root);
    let submission = manifest
        .strip_prefix(root)
        .ok()
        .and_then(|p| p.components().next())
        .map_or_else(
            || "unknown".to_owned(),
            |c| c.as_os_str().to_string_lossy().into_owned(),
        );

    let mut out = Vec::new();
    for node in doc.descendants() {
        if doc.element_name(node).is_none_or(|n| n.local != "TEST") {
            continue;
        }
        let attr = |k: &str| doc.attribute(node, k).map(str::to_owned);
        let (Some(id), Some(kind), Some(uri)) =
            (attr("ID"), attr("TYPE"), attr("URI"))
        else {
            return Err(format!(
                "{}: a TEST is missing ID, TYPE or URI",
                manifest.display()
            ));
        };
        let Some(kind) = TestType::parse(&kind) else {
            return Err(format!("{}: unknown TYPE {kind}", manifest.display()));
        };
        out.push(TestCase {
            id,
            kind,
            path: base.join(&uri),
            submission: submission.clone(),
            recommendation: attr("RECOMMENDATION"),
            version: attr("VERSION"),
            edition: attr("EDITION"),
            namespace: attr("NAMESPACE").as_deref() != Some("no"),
            entities: attr("ENTITIES"),
        });
    }
    Ok(out)
}

/// Drop an XML declaration from a fragment.
///
/// A declaration is only legal at the very start of a document, so it
/// cannot survive being wrapped in a synthetic root.
fn strip_prolog(text: &str) -> &str {
    let trimmed = text.trim_start();
    trimmed.strip_prefix("<?xml").map_or(trimmed, |rest| {
        rest.find("?>").map_or(trimmed, |end| &rest[end + 2..])
    })
}
