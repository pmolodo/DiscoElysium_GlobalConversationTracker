// SPDX-License-Identifier: MIT
//! A unified diff, applied.
//!
//! ## Why this is here rather than a call to git
//!
//! The C# side creates and applies these by shelling out to `git diff` and `git apply`,
//! which is a reasonable thing for a packer on a developer's machine to do and a wrong
//! thing for a FORMAT LAYER to do. This layer's whole claim is that it is the one
//! definition of what these files are; a definition that only works where someone has
//! installed git is a definition with a footnote.
//!
//! ## And why it is `diffy` rather than a parser written here
//!
//! Unified diff looks like a format anyone could read in eighty lines, and the eighty
//! lines are where the mistakes live: a hunk that only adds is written with a count of
//! zero and a start line meaning the gap AFTER it; the marker for a missing final newline
//! is a line that is not content; a context line whose content is empty is a single space
//! that anything trimming whitespace will have eaten. Each is a way to apply a diff
//! slightly wrong and produce a file nobody wrote.
//!
//! ## Creating one, and whose edit script it is
//!
//! [`create`] emits `diffy`'s. A diff is not unique - several edit scripts turn one file
//! into another and all of them are correct - so the bytes here are not the bytes git
//! would have written for the same pair, and nothing depends on them being. What a diff
//! has to do is APPLY to the baseline it was taken against and produce the target, which
//! is the property the tests hold it to.
//!
//! ONE LINE OF CONTEXT, which is what the committed diffs carry. A save's non-JSON members
//! are one statement per line with no structure around them, so three lines of context
//! triples the size of a diff without making it any easier to read.
//!
//! ## The first line is what this format's `_base` is
//!
//! A unified diff already has somewhere to say what it is a diff of - the `---` line - so
//! it says it there rather than needing a header it has no room for. The path is written
//! RELATIVE TO THE DIFF, like every other `_base` in this repository, and WITHOUT the `a/`
//! and `b/` prefixes: those are git's own marker for two versions of one working tree, and
//! here the two names are two real files, one of which a reader has to be able to open.
//!
//! ## Line endings
//!
//! Normalised to `\n` before anything is compared, because the diff was taken over
//! normalised text and a save that arrived with CRLF would otherwise match no context line
//! at all.

/// How many unchanged lines a hunk carries either side of what it changes.
const CONTEXT: usize = 1;

/// Why a diff could not be applied.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TextDiffFault {
    /// It is not a unified diff.
    #[error("{0} is not a unified diff: {1}")]
    Malformed(String, String),
    /// It expected a line the baseline does not have there.
    ///
    /// The one that matters. A diff applied against the wrong baseline would produce a
    /// file that is neither, and every later reading of it would be of something nobody
    /// wrote.
    #[error("{0} is a diff of something else: {1}")]
    Mismatch(String, String),
    /// It says nothing about what it is a diff of.
    #[error("{0} is a diff and its first line names nothing to be a diff of")]
    Baseless(String),
}

/// Applies a unified diff to the text it was taken against.
///
/// `context` names the member in any fault, since a caller applying several needs to know
/// which one.
///
/// # Errors
///
/// Where the patch is not a unified diff, or where it expects a line the baseline does not
/// have - which means it is a diff of something else.
pub fn apply(baseline: &str, patch: &str, context: &str) -> Result<String, TextDiffFault> {
    let normalised = normalise(patch);
    let parsed = diffy::Patch::from_str(&normalised)
        .map_err(|why| TextDiffFault::Malformed(context.to_string(), why.to_string()))?;

    diffy::apply(&normalise(baseline), &parsed)
        .map_err(|why| TextDiffFault::Mismatch(context.to_string(), why.to_string()))
}

/// The diff that turns one member into another, or nothing where they are the same.
///
/// `base` is where the file this is a diff of lives, relative to where the diff itself
/// will, and `name` is what the member is called in the save being written. They are the
/// diff's two filenames, and the first of them is this format's `_base` - see the note
/// above on why the `a/` and `b/` prefixes are not written.
///
/// NOTHING WHERE THEY MATCH, rather than an empty diff, because that is the answer the
/// manifest needs: a member that did not change is inherited and has no file beside it.
#[must_use]
pub fn create(base: &str, name: &str, baseline: &str, target: &str) -> Option<String> {
    let was = normalise(baseline);
    let wanted = normalise(target);
    if was == wanted {
        return None;
    }

    let patch = diffy::DiffOptions::new()
        .set_context_len(CONTEXT)
        .set_original_filename(base.to_string())
        .set_modified_filename(name.to_string())
        .create_patch(&was, &wanted);

    Some(patch.to_string())
}

/// What a diff says it is a diff of, which is the path on its first line.
///
/// # Errors
///
/// Where the text is not a unified diff, or where it names nothing to be a diff of.
pub fn base_of(patch: &str, context: &str) -> Result<String, TextDiffFault> {
    let normalised = normalise(patch);
    let parsed = diffy::Patch::from_str(&normalised)
        .map_err(|why| TextDiffFault::Malformed(context.to_string(), why.to_string()))?;

    parsed
        .original()
        .map(str::to_string)
        .ok_or_else(|| TextDiffFault::Baseless(context.to_string()))
}

/// The same text with the line endings the diff was taken over.
fn normalise(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const BASELINE: &str = "one\ntwo\nthree\nfour\n";

    fn applied(patch: &str) -> String {
        apply(BASELINE, patch, "a test member").expect("it applies")
    }

    #[test]
    fn a_changed_line_is_replaced_and_the_rest_is_kept() {
        let patch = "--- a/f\n+++ b/f\n@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n";

        assert_eq!(applied(patch), "one\nTWO\nthree\nfour\n");
    }

    #[test]
    fn an_added_line_lands_where_the_hunk_puts_it() {
        let patch = "--- a/f\n+++ b/f\n@@ -2,1 +2,2 @@\n two\n+extra\n";

        assert_eq!(applied(patch), "one\ntwo\nextra\nthree\nfour\n");
    }

    #[test]
    fn a_removed_line_is_gone_and_nothing_else_moves() {
        let patch = "--- a/f\n+++ b/f\n@@ -1,3 +1,2 @@\n one\n-two\n three\n";

        assert_eq!(applied(patch), "one\nthree\nfour\n");
    }

    #[test]
    fn several_hunks_apply_in_order() {
        let patch = concat!(
            "--- a/f\n+++ b/f\n",
            "@@ -1,2 +1,2 @@\n-one\n+ONE\n two\n",
            "@@ -3,2 +3,2 @@\n three\n-four\n+FOUR\n",
        );

        assert_eq!(applied(patch), "ONE\ntwo\nthree\nFOUR\n");
    }

    /// The one that matters: a diff of something else must not half-apply.
    #[test]
    fn a_diff_of_a_different_file_is_refused_rather_than_forced() {
        let patch = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n one\n-something else\n+TWO\n";

        let refused = apply(BASELINE, patch, "at-trashcan.states.lua").expect_err("refused");

        assert!(
            matches!(refused, TextDiffFault::Mismatch(_, _)),
            "{refused}"
        );
        assert!(
            refused.to_string().contains("at-trashcan.states.lua"),
            "a fault names the member it is about: {refused}",
        );
    }

    #[test]
    fn a_malformed_hunk_is_refused() {
        let patch = "--- a/f\n+++ b/f\n@@ what @@\n-two\n+TWO\n";

        let refused = apply(BASELINE, patch, "a test member").expect_err("refused");

        assert!(
            matches!(refused, TextDiffFault::Malformed(_, _)),
            "{refused}"
        );
    }

    /// Text carrying no hunks changes nothing, which is not the same as being refused.
    ///
    /// Worth pinning because it is the tolerant reading and it could go either way: a
    /// diff is a list of hunks, and a list of none of them is an empty change rather than
    /// a malformed document. It is also what lets a patch carry a covering note.
    #[test]
    fn text_with_no_hunks_in_it_leaves_the_baseline_alone() {
        assert_eq!(applied("not a diff at all"), BASELINE);
        assert_eq!(applied("--- a/f\n+++ b/f\n"), BASELINE);
    }

    /// Where the base sits relative to a diff of it, which is a step up and across.
    const BENEATH: &str = "../base.ntwtf/base.states.lua";

    /// The property that matters about a created diff, and the only one.
    #[test]
    fn what_is_created_turns_the_baseline_into_the_target() {
        let target = "one\nTWO\nthree\nfour\nfive\n";

        let patch = create(BENEATH, "a.states.lua", BASELINE, target).expect("they differ");

        assert_eq!(
            apply(BASELINE, &patch, "a.states.lua").expect("applies"),
            target
        );
    }

    /// The first line is this format's `_base`, so a reader can find the file from the diff.
    #[test]
    fn a_created_diff_names_what_it_is_a_diff_of_and_what_it_makes() {
        let patch =
            create(BENEATH, "at-trashcan.states.lua", BASELINE, "one\n").expect("they differ");

        assert!(
            patch.starts_with(&format!("--- {BENEATH}\n+++ at-trashcan.states.lua\n")),
            "{patch}",
        );
        assert_eq!(
            base_of(&patch, "a test member").expect("it names one"),
            BENEATH
        );
    }

    /// A base whose name would confuse the parser survives the round trip.
    ///
    /// Worth pinning because a real save's name carries a timestamp with spaces in it, and
    /// an unquoted filename ends at the first space - so the writer has to quote and the
    /// reader has to unquote, or the base comes back as the first word of one.
    #[test]
    fn a_base_whose_name_has_spaces_in_it_reads_back_whole() {
        let spaced = "../WHIRLING (8_31_2026 8-25-00 PM).ntwtf/x.states.lua";

        let patch = create(spaced, "a.states.lua", BASELINE, "one\n").expect("they differ");

        assert_eq!(
            base_of(&patch, "a test member").expect("it names one"),
            spaced
        );
    }

    /// A member that did not change has no diff, which is what makes it inheritable.
    #[test]
    fn two_of_the_same_text_produce_no_diff_at_all() {
        assert_eq!(create(BENEATH, "a.states.lua", BASELINE, BASELINE), None);
    }

    /// Line endings are not a change, because the diff is taken over normalised text.
    #[test]
    fn the_same_text_with_other_line_endings_produces_no_diff() {
        assert_eq!(
            create(
                BENEATH,
                "a.states.lua",
                BASELINE,
                "one\r\ntwo\r\nthree\r\nfour\r\n"
            ),
            None,
        );
    }

    /// A diff with no first line names nothing, and is refused rather than guessed at.
    #[test]
    fn a_diff_that_names_no_base_is_refused() {
        let refused = base_of(
            "@@ -1,3 +1,3 @@\n one\n-two\n+TWO\n three\n",
            "a test member",
        )
        .expect_err("refused");

        assert!(matches!(refused, TextDiffFault::Baseless(_)), "{refused}");
    }

    /// A save that arrived with CRLF still matches a diff taken over LF.
    #[test]
    fn carriage_returns_do_not_stop_a_hunk_matching() {
        let patch = "--- a/f\r\n+++ b/f\r\n@@ -1,3 +1,3 @@\r\n one\r\n-two\r\n+TWO\r\n three\r\n";

        assert_eq!(
            apply("one\r\ntwo\r\nthree\r\nfour\r\n", patch, "a test member").expect("applies"),
            "one\nTWO\nthree\nfour\n",
        );
    }
}
