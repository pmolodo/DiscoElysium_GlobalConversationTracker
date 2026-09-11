// SPDX-License-Identifier: MIT
//! A document that is a diff of another, followed to the whole document it describes.
//!
//! WHAT A DIFF IS A DIFF OF IS WRITTEN IN THE DIFF, in `_base`, relative to itself. That is
//! what lets a chain be walked from either end without a manifest beside it: the fixture
//! names what it changes, and what it changes may name what IT changes, down to a document
//! that carries no base and is therefore whole.
//!
//! THE FILE ON DISK IS NOT WHAT THE READER GETS. A reader of one of these formats is handed
//! the resolved document, so nothing downstream has to know whether the fixture it was
//! pointed at was written whole or as a change to something else.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::header::is_stamped;
use super::json_diff;

/// Why a document could not be resolved to a whole one.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ResolveFault {
    /// It is not there.
    #[error("{path} is not there{}", what_named_it(.named_by))]
    Missing {
        path: String,
        named_by: Option<String>,
    },
    /// It is there and is not JSON.
    #[error("{path} will not parse as JSON: {why}")]
    Malformed { path: String, why: String },
    /// It is a diff and says nothing about what it is a diff of.
    #[error(
        "{path} is a diff and names no {} to be a diff of",
        super::header::BASE_KEY
    )]
    Baseless { path: String },
    /// It is a diff of something that is a diff of it.
    #[error("the diffs run in a circle: {0}")]
    Circular(String),
    /// It is a diff that will not apply to what it is a diff of.
    #[error("{path}: {why}")]
    Unapplicable { path: String, why: String },
}

/// Which file named the one that is missing, for the message.
fn what_named_it(named_by: &Option<String>) -> String {
    match named_by {
        Some(who) => format!(", and '{who}' is a diff of it"),
        None => String::new(),
    }
}

/// One document, with every diff between it and a whole document applied.
///
/// # Errors
///
/// Where the file or one of its bases is missing, will not parse, is a diff that names no
/// base, or the chain returns to a file it has already opened.
pub fn document(path: &Path) -> Result<Value, ResolveFault> {
    let mut walked = HashSet::new();
    let mut chain = Vec::new();
    resolve(path, None, &mut walked, &mut chain)
}

/// The same, tracking what has been opened so a circle is a message rather than a hang.
fn resolve(
    path: &Path,
    named_by: Option<&Path>,
    walked: &mut HashSet<PathBuf>,
    chain: &mut Vec<String>,
) -> Result<Value, ResolveFault> {
    let shown = path.display().to_string();
    chain.push(shown.clone());
    if !walked.insert(path.to_path_buf()) {
        return Err(ResolveFault::Circular(chain.join(" -> ")));
    }

    let text = std::fs::read_to_string(path).map_err(|_| ResolveFault::Missing {
        path: shown.clone(),
        named_by: named_by.map(|who| who.display().to_string()),
    })?;
    let document: Value = serde_json::from_str(&text).map_err(|why| ResolveFault::Malformed {
        path: shown.clone(),
        why: why.to_string(),
    })?;

    // A DOCUMENT THAT NAMES NO FORMAT IS A WHOLE ONE. Only a diff carries a header today,
    // and de-xz48.5 is where the documents that carry none gain one - at which point this
    // asks whether the header names a DIFF rather than whether there is a header at all.
    if !is_stamped(&document) {
        return Ok(document);
    }

    let base = json_diff::base_of(&document).ok_or(ResolveFault::Baseless {
        path: shown.clone(),
    })?;
    let beneath = path.parent().unwrap_or_else(|| Path::new(".")).join(base);

    let whole = resolve(&beneath, Some(path), walked, chain)?;
    json_diff::apply(&whole, &document).map_err(|why| ResolveFault::Unapplicable {
        path: shown,
        why: why.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// A folder of this test's own, since what is being tested is reading files.
    struct Folder(PathBuf);

    impl Folder {
        fn new(named: &str) -> Self {
            let path = std::env::temp_dir().join(format!("gct-resolve-{named}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("a folder to write in");
            Self(path)
        }

        fn write(&self, name: &str, document: &Value) -> PathBuf {
            let path = self.0.join(name);
            std::fs::write(&path, serde_json::to_string_pretty(document).expect("json"))
                .expect("the fixture writes");
            path
        }
    }

    impl Drop for Folder {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn diff(base: &str, changes: Value) -> Value {
        let mut patch = json_diff::FORMAT.stamp();
        patch.insert(super::super::header::BASE_KEY.to_string(), base.into());
        patch.insert(json_diff::CHANGES_KEY.to_string(), changes);
        Value::Object(patch)
    }

    #[test]
    fn a_whole_document_resolves_to_itself() {
        let folder = Folder::new("whole");
        let path = folder.write("state.json", &json!({ "version": 4, "orbs": ["one"] }));

        assert_eq!(
            document(&path).expect("it resolves"),
            json!({ "version": 4, "orbs": ["one"] })
        );
    }

    #[test]
    fn a_diff_resolves_to_what_it_changes() {
        let folder = Folder::new("one-step");
        folder.write("base.json", &json!({ "a": 1, "b": { "c": 2 } }));
        let path = folder.write("top.json", &diff("base.json", json!({ "b": { "c": 9 } })));

        assert_eq!(
            document(&path).expect("it resolves"),
            json!({ "a": 1, "b": { "c": 9 } })
        );
    }

    #[test]
    fn a_diff_of_a_diff_resolves_through_both() {
        let folder = Folder::new("chain");
        folder.write("base.json", &json!({ "a": 1 }));
        folder.write("middle.json", &diff("base.json", json!({ "b": 2 })));
        let path = folder.write("top.json", &diff("middle.json", json!({ "c": 3 })));

        assert_eq!(
            document(&path).expect("it resolves"),
            json!({ "a": 1, "b": 2, "c": 3 })
        );
    }

    #[test]
    fn a_diff_of_something_that_is_not_there_says_what_named_it() {
        let folder = Folder::new("missing");
        let path = folder.write("top.json", &diff("nowhere.json", json!({})));

        let fault = document(&path).unwrap_err().to_string();

        assert!(fault.contains("nowhere.json"));
        assert!(fault.contains("top.json"));
    }

    #[test]
    fn diffs_that_are_diffs_of_each_other_are_refused() {
        let folder = Folder::new("circle");
        folder.write("round.json", &diff("trip.json", json!({})));
        let path = folder.write("trip.json", &diff("round.json", json!({})));

        assert!(
            document(&path)
                .unwrap_err()
                .to_string()
                .contains("run in a circle")
        );
    }

    #[test]
    fn a_diff_that_names_no_base_is_refused() {
        let folder = Folder::new("baseless");
        let mut patch = json_diff::FORMAT.stamp();
        patch.insert(json_diff::CHANGES_KEY.to_string(), json!({ "a": 1 }));
        let path = folder.write("top.json", &Value::Object(patch));

        assert!(
            document(&path)
                .unwrap_err()
                .to_string()
                .contains(super::super::header::BASE_KEY)
        );
    }
}
