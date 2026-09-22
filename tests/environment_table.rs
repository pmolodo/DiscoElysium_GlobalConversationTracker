// SPDX-License-Identifier: MIT
//! Does `docs/environment.md` still list the variables the code actually asks for - and is
//! every variable the code reads one of ours or one somebody else owns?
//!
//! TWO QUESTIONS, ONE FILE, because they are two halves of the same claim and they share how a
//! name is found in a file. The table answers "what can be set"; the second test answers
//! "is this ours", which nothing asked until six variables under a prefix of their own turned
//! out to be invisible to the first - see [`every_name_the_code_reads_is_ours_or_somebody_elses`].
//!
//! ## Why this is a test and not a generator
//!
//! The table names every `DEGCT_` variable and the files that name it. It was pasted in by hand
//! from a script, and the doc said what would happen: "Nothing enforces the table, and a stale
//! one is worse than none: a reader who trusts it will look for a variable that has been
//! renamed." It went five variables stale, and the count above it said 45 against an actual 50,
//! for weeks, with nothing to notice. See de-yqjr.
//!
//! WHAT IS WANTED IS NOT GENERATION BUT ENFORCEMENT. A build script that rewrote a tracked file
//! would dirty the working tree on every build - which shows up in every measurement's log name
//! as `-dirty` - and cargo's own guidance is that a build script writes to `OUT_DIR` and nowhere
//! else. A human pasting rows is fine as long as something fails when they forget, so this is
//! that something: it regenerates the rows, compares, and prints the exact block to paste.
//!
//! ## Why in Rust, and why the Python is gone
//!
//! Shelling out to `tools/env-table.py` would make a Python interpreter a build dependency of
//! the crate for everyone forever, and it would have to find the right one - `python` is not
//! `python3` everywhere. Running the tests already needs cargo and nothing else.
//!
//! TWO IMPLEMENTATIONS OF "WHICH FILES NAME THIS VARIABLE" WOULD BE THE SAME DRIFT ONE LEVEL UP,
//! so the script was deleted rather than kept beside this.
//!
//! ## Why it is still here with five rows to guard - de-3dx9.8
//!
//! The list went from fifty variables to five, and the obvious question is whether a test is
//! worth keeping over a table a person can read at a glance. It is, and the run that shrank the
//! list is the argument: this test caught three things in it that nobody was looking for.
//!
//! A ROW THAT SURVIVED ON A SENTENCE. Five variables stayed in the table after nothing read them,
//! held there by an example in a helper's own header - because a MENTION is what puts a row here,
//! and a helper naming a variable to show how the door works is a mention.
//!
//! A ROW FOR A VARIABLE THAT NEVER EXISTED. `command.env("GIT_INDEX_FILE", ..)` sets a variable
//! GIT owns, and reading it as one of ours invented a `DEGCT_GIT_INDEX_FILE` that had a row for
//! as long as the table has had rows.
//!
//! TEN ROWS OF A SCRIPT TALKING TO ITSELF, which is what [`is_a_script_local`] now tells apart.
//!
//! None of those is the drift the test was written for, and none would have been found by
//! reading the table - they are what a reader BELIEVES when a table looks authoritative. A short
//! list is a reason to keep a cheap guard rather than a reason to drop one: the shorter it is,
//! the more each row is trusted.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

/// The doc this checks, which would otherwise count every variable it lists.
const TABLE: &str = "docs/environment.md";

/// The files ABOUT the variables, whose own patterns and prose name them as examples.
///
/// A file whose subject is the list cannot also be evidence for it: every name it uses to explain
/// itself would become a row, and a name it stopped explaining would silently leave one.
///
/// SIX OF THEM. This file and `tools/survey-env.py` look for variables and name them as patterns
/// to look for. The four DOORS - one per language that has one - take a name as a parameter and
/// read nothing of their own, so every name in them is showing how the door works. Five rows
/// survived in the table on the strength of an example in one of those headers, after nothing
/// read the variables any more.
const ABOUT: [&str; 6] = [
    "tests/environment_table.rs",
    "tools/survey-env.py",
    "src/core/env.rs",
    "tools/degct-env.sh",
    "tools/DegctEnv.psm1",
    "tools/GameAutomation/DegctEnv.cs",
];

/// What a file has to end in to be worth reading: the languages that have a helper, plus the
/// docs, which name variables in prose, plus the build's own files.
///
/// THE BUILD FILES ARE HERE BECAUSE THEY WERE NOT. A variable naming the game to build against
/// lived in `Directory.Build.props` and in the scripts at the root, in none of which this could
/// look, so it stayed under a prefix of its own long after the rule - and the table said six
/// while the code asked for twelve. A suffix that is cheap to read is cheap to include; what is
/// expensive is a file nobody thought to look in. See de-ej10.
const SUFFIXES: [&str; 9] = [
    "rs", "py", "sh", "psm1", "ps1", "props", "targets", "cs", "md",
];

/// The prefix on every variable this project defines.
const PREFIX: &str = "DEGCT_";

/// Names somebody ELSE owns, which a raw read may spell out because they are not ours to name.
///
/// The list is short on purpose and every entry is a name that exists whether this project does
/// or not: the shell's and the OS's, cargo's, and the one dotnet's test host sets. A name that
/// is not here and is not prefixed is a variable somebody invented, which is what
/// [`every_name_the_code_reads_is_ours_or_somebody_elses`] is for.
const FOREIGN: [&str; 7] = [
    "DOTNET_HOST_PATH",
    "HOME",
    "LOCALAPPDATA",
    "OUT_DIR",
    "PATH",
    "SystemRoot",
    "USERPROFILE",
];

/// A namespace somebody else owns, rather than a name: everything cargo sets a build or a test
/// binary up with. `CARGO_MANIFEST_DIR`, `CARGO_PKG_VERSION` and one `CARGO_BIN_EXE_` per
/// binary are read here, and listing a family whose membership cargo decides would be a list
/// that goes short every time a test asks for another binary.
const FOREIGN_NAMESPACE: &str = "CARGO_";

/// The ways of reading the environment that apply NO prefix, with what follows each one.
///
/// Our doors are not here: they take a bare name and put the prefix on, which is the whole
/// difference. `std::env::var` is here and `env::var` is not, for that reason.
const RAW: [&str; 8] = [
    "$env:",
    "Environment.GetEnvironmentVariable(",
    "Environment.SetEnvironmentVariable(",
    "std::env::var(",
    "std::env::var_os(",
    "env!(",
    "os.environ[",
    "os.environ.get(",
];

/// How C# holds the name of a variable: a constant whose own name ends in this.
const HOLDER: &str = "Variable = \"";

/// Calls that take a BARE name as a quoted first argument.
///
/// Not `std::env::var`, which reads somebody else's name under its own spelling - the `env::`
/// entries below are matched only where `std::` does not precede them.
///
/// The C# shapes carry their type's name, unlike the others. `Get(` and `IsSet(` on their own
/// are words a C# file uses for a hundred other things, and a pattern that matched them would
/// put a row in the table for every one.
const QUOTED: [&str; 17] = [
    "DegctEnv.Get(",
    "DegctEnv.IsSet(",
    "DegctEnv.Qualified(",
    "env::var(",
    "env::is_set(",
    "env::number(",
    "env::pass(",
    "env::qualified(",
    "from_env(",
    "from_env_i32(",
    "numbers(",
    "env(",
    "env_is_set(",
    "env_int(",
    "env_list(",
    "qualified(",
    "env_for_child(",
];

/// Calls that take a BARE name as an unquoted argument: bash and PowerShell.
const BARE: [&str; 7] = [
    "degct_env_is_set ",
    "degct_env_set ",
    "degct_env ",
    "Get-DegctEnv ",
    "Test-DegctEnv ",
    "Set-DegctEnv ",
    "ConvertTo-DegctEnvName ",
];

#[test]
fn the_environment_doc_lists_what_the_code_asks_for() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(found) = rows(&root) else {
        // NO GIT, NO LIST OF FILES, NO TEST. This reads what git tracks rather than walking the
        // tree, so that build output and a downloaded game's files are never in it.
        return;
    };

    let doc = std::fs::read_to_string(root.join(TABLE)).expect("the environment doc reads");
    let listed: Vec<String> = doc
        .lines()
        .filter(|line| line.starts_with("| `DEGCT_"))
        .map(str::to_string)
        .collect();

    assert_eq!(
        listed,
        found,
        "\n\ndocs/environment.md no longer lists what the code asks for.\n\
         Missing from the doc:\n{}\nIn the doc and not in the code:\n{}\n\
         Paste this in place of the table's rows:\n\n{}\n",
        only_in(&found, &listed),
        only_in(&listed, &found),
        found.join("\n")
    );

    // AND THE COUNT ABOVE IT, which drifted too and is the part a reader believes without
    // checking. It is written as a sentence rather than a field, so it is found by its shape.
    let said = doc
        .lines()
        .find_map(|line| {
            let (before, _) = line.split_once(" variables.")?;
            before.rsplit(' ').next()?.parse::<usize>().ok()
        })
        .expect("the doc says how many variables there are");
    assert_eq!(
        said,
        found.len(),
        "docs/environment.md says {said} variables and lists {}",
        found.len()
    );
}

/// Is every name a raw read spells out either ours or somebody else's?
///
/// ## What this asks that the table does not
///
/// The table enforces one direction: every `DEGCT_` name the code asks for has a row. Nothing
/// asked the other - whether a name the code asks for is ONE OF OURS AT ALL - so a variable
/// invented under a prefix of its own was invisible to it, and six of them were, for as long as
/// the rule had existed. See de-ej10 for the six and de-dnqe for this.
///
/// ## What it can see, and what it cannot
///
/// A name is visible where it is WRITTEN DOWN: at a raw read, or as the literal a C# constant
/// holds. Where a name is computed, this says nothing, and MSBuild is out of reach altogether -
/// `$(NAME)` is a property reference and an environment read at once, with nothing to tell them
/// apart.
///
/// A CONSTANT MAY HOLD A BARE NAME, which is the shape to prefer: the door puts the prefix on,
/// so the file holds `"NO_RUN_LOG"` and reads `DEGCT_NO_RUN_LOG`. That is why a holder is
/// accepted when the file passes it to a door, and challenged when nothing does - a bare name
/// nothing prefixes is read bare, and a full name under some other prefix looks exactly the
/// same from here.
#[test]
fn every_name_the_code_reads_is_ours_or_somebody_elses() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let Some(files) = tracked(&root) else {
        return;
    };

    let mut wrong: Vec<String> = Vec::new();
    for path in files {
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else {
            continue;
        };

        for (name, how) in raw_reads(&text) {
            if is_ours_or_foreign(&name) {
                continue;
            }
            wrong.push(format!(
                "{path}: {how} reads '{name}', which is neither one of ours nor a name \
                 somebody else owns - prefix it {PREFIX} through the door for this language, \
                 or add it to FOREIGN with what owns it"
            ));
        }

        for (name, holder) in holders(&text) {
            if is_ours_or_foreign(&name) || goes_through_the_door(&text, &holder) {
                continue;
            }
            wrong.push(format!(
                "{path}: {holder} holds '{name}', which nothing here passes to a door - so it \
                 is read exactly as written, and a name read as written has to carry {PREFIX}"
            ));
        }
    }

    assert!(wrong.is_empty(), "\n{}", wrong.join("\n"));
}

/// Whether a name is prefixed as ours or listed as somebody else's.
fn is_ours_or_foreign(name: &str) -> bool {
    name.starts_with(PREFIX) || name.starts_with(FOREIGN_NAMESPACE) || FOREIGN.contains(&name)
}

/// The names a text reads WITHOUT a prefix being applied, each with the shape that read it.
///
/// Only where the name is written at the call: an identifier is held somewhere else and is
/// [`holders`]' business.
fn raw_reads(text: &str) -> Vec<(String, &'static str)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for shape in RAW {
        for at in matches(text, shape) {
            if at > 0 && is_word(bytes[at - 1]) {
                continue;
            }
            let mut i = skip_spaces(bytes, at + shape.len());
            // Everything but PowerShell's `$env:NAME` quotes the name.
            if shape != "$env:" {
                if bytes.get(i) != Some(&b'"') {
                    continue;
                }
                i += 1;
            }
            let Some(name) = literal_at(bytes, i) else {
                continue;
            };
            found.push((name, shape));
        }
    }
    found
}

/// The names C# constants hold, each with the constant's own name.
fn holders(text: &str) -> Vec<(String, String)> {
    let bytes = text.as_bytes();
    let mut found = Vec::new();
    for at in matches(text, HOLDER) {
        let Some(name) = literal_at(bytes, at + HOLDER.len()) else {
            continue;
        };
        // The constant's own name, back from the `Variable` this matched on.
        let mut from = at + "Variable".len();
        while from > 0 && is_word(bytes[from - 1]) {
            from -= 1;
        }
        found.push((
            name,
            String::from_utf8_lossy(&bytes[from..at + "Variable".len()]).into_owned(),
        ));
    }
    found
}

/// Whether this text hands `holder` to the C# door, which is what puts the prefix on.
fn goes_through_the_door(text: &str, holder: &str) -> bool {
    matches(text, "DegctEnv.").iter().any(|at| {
        let rest = &text[*at..];
        let until = rest.find(')').unwrap_or(rest.len());
        rest[..until].contains(holder)
    })
}

/// The quoted name starting at `from`: what an environment variable may be called, in any case,
/// since a foreign one need not shout.
fn literal_at(bytes: &[u8], from: usize) -> Option<String> {
    if !(bytes.get(from)?.is_ascii_alphabetic() || bytes[from] == b'_') {
        return None;
    }
    let mut end = from;
    while end < bytes.len() && (bytes[end].is_ascii_alphanumeric() || bytes[end] == b'_') {
        end += 1;
    }
    (end - from >= 2).then(|| String::from_utf8_lossy(&bytes[from..end]).into_owned())
}

/// The tracked files worth reading, or `None` where git cannot say.
fn tracked(root: &Path) -> Option<Vec<String>> {
    let listed = Command::new("git")
        .arg("ls-files")
        .current_dir(root)
        .output()
        .ok()?;
    if !listed.status.success() {
        return None;
    }

    Some(
        String::from_utf8_lossy(&listed.stdout)
            .lines()
            .filter(|path| *path != TABLE && !ABOUT.contains(path))
            .filter(|path| {
                SUFFIXES
                    .iter()
                    .any(|suffix| path.ends_with(&format!(".{suffix}")))
            })
            .map(str::to_string)
            .collect(),
    )
}

/// Every name the tracked files ask for, as the table's rows, or `None` where git cannot say.
fn rows(root: &Path) -> Option<Vec<String>> {
    let mut readers: BTreeMap<String, BTreeSet<(String, bool)>> = BTreeMap::new();
    for path in tracked(root)? {
        let path = path.as_str();
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        // ONLY A SHELL FILE CAN TALK TO ITSELF THIS WAY. `NAME=value` in a doc or a code
        // sample is an instruction to a READER - here is what to set before running this -
        // and reading it as a script's own local hid `DEGCT_INGAME_TESTS`, whose every
        // mention is an example of setting it.
        let assigned = if assigns_like_a_script(path) {
            assignments_in(&text)
        } else {
            BTreeSet::new()
        };
        for name in names_in(&text) {
            let mine = assigned.contains(&name);
            readers
                .entry(name)
                .or_default()
                .insert((path.to_string(), mine));
        }
    }

    Some(
        readers
            .into_iter()
            .filter(|(_, files)| !is_a_script_local(files))
            .map(|(name, files)| {
                let files: Vec<String> =
                    files.iter().map(|(path, _)| format!("`{path}`")).collect();
                format!("| `{PREFIX}{name}` | {} |", files.join(", "))
            })
            .collect(),
    )
}

/// Whether every file that names this one also ASSIGNS it, which makes it a script's own
/// variable rather than an option anybody can set.
///
/// ## Why the table must not list these
///
/// The DEGCT_ rule covers a shell script's locals as well as its exports, and for a good reason:
/// the collision that started the rule was a local, and `GROUPS` is a built-in array whatever a
/// script meant by it. So a script's working variables carry the prefix and always will.
///
/// But this table's job is to say WHAT CAN BE SET - the options that reach the project from
/// outside - and a variable a script assigns before it reads, never exports, and nothing else
/// mentions, cannot be one. Listing them made the table mostly noise: of seventeen rows, ten
/// were one shell script talking to itself, which is exactly the state that makes a reader stop
/// trusting the other seven.
///
/// A NAME SOME OTHER FILE READS IS NOT THIS, however many scripts assign it. `DEGCT_RUN_LOG_DIR`
/// is set by `measure-symbolic.sh` for a child and read by `measurement_common.py`, so it
/// crosses a boundary and stays.
fn is_a_script_local(files: &BTreeSet<(String, bool)>) -> bool {
    !files.is_empty() && files.iter().all(|(_, assigns)| *assigns)
}

/// Whether a file is one whose `NAME=value` is an assignment rather than an example.
///
/// The shells, and nothing else. Rust and Python do not assign into the environment this way at
/// all, and in a doc or a comment the shape is how a reader is TOLD to set something.
fn assigns_like_a_script(path: &str) -> bool {
    [".sh", ".psm1", ".ps1"]
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

/// Every bare name this text ASSIGNS, as a shell script assigns one: `DEGCT_NAME=` at the start
/// of a word.
fn assignments_in(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut found = BTreeSet::new();
    for at in matches(text, PREFIX) {
        if at > 0 && is_word(bytes[at - 1]) {
            continue;
        }
        let Some(name) = name_at(bytes, at + PREFIX.len()) else {
            continue;
        };
        // `NAME=` and `NAME+=`, but not `NAME==` or `NAME=~`, which are comparisons.
        let mut after = at + PREFIX.len() + name.len();
        if bytes.get(after) == Some(&b'+') {
            after += 1;
        }
        if bytes.get(after) == Some(&b'=') && bytes.get(after + 1) != Some(&b'=') {
            found.insert(name);
        }
    }
    found
}

/// Every bare name `text` asks for, by any of the ways there are to ask.
fn names_in(text: &str) -> BTreeSet<String> {
    let bytes = text.as_bytes();
    let mut found = BTreeSet::new();

    // SPELLED OUT IN FULL, anywhere - code, a comment, a doc. Not where it is part of a longer
    // word, which is how the scratch prefix `DEGCTT_` and the doubled `DEGCT_DEGCT_` stay out.
    for at in matches(text, PREFIX) {
        if at > 0 && is_word(bytes[at - 1]) {
            continue;
        }
        if let Some(name) = name_at(bytes, at + PREFIX.len()) {
            found.insert(name);
        }
    }

    for call in QUOTED {
        for at in matches(text, call) {
            if call.starts_with("env::") && at >= 5 && &text[at - 5..at] == "std::" {
                continue;
            }
            if at > 0 && is_word(bytes[at - 1]) {
                continue;
            }
            // A METHOD CALL IS NOT OUR DOOR. `command.env("GIT_INDEX_FILE", ..)` sets a variable
            // GIT owns, on a child this project starts, and reading it as `env(` invented a
            // DEGCT_GIT_INDEX_FILE that nothing has ever defined. Ours is `env::pass`, which
            // applies the prefix - see `src/core/env.rs`, whose whole purpose is that the two
            // cannot be confused at a call site.
            if at > 0 && bytes[at - 1] == b'.' {
                continue;
            }
            if call == "env_for_child(" {
                found.extend(keywords(&text[at + call.len()..]));
                continue;
            }
            let mut i = skip_spaces(bytes, at + call.len());
            if bytes.get(i) != Some(&b'"') {
                continue;
            }
            i += 1;
            let Some(name) = name_at(bytes, i) else {
                continue;
            };
            if bytes.get(i + name.len()) != Some(&b'"') {
                continue;
            }
            // PYTHON'S FOREIGN DOOR reads a name somebody else owns under its own spelling, so
            // what it names is not one of ours.
            let rest = &text[i + name.len()..];
            let until = rest.find(')').unwrap_or(rest.len());
            if rest[..until].contains("foreign=True") {
                continue;
            }
            found.insert(name);
        }
    }

    for call in BARE {
        for at in matches(text, call) {
            if at > 0 && is_word(bytes[at - 1]) {
                continue;
            }
            if let Some(name) = name_at(bytes, at + call.len()) {
                found.insert(name);
            }
        }
    }

    // A NAME THAT ALREADY CARRIES THE PREFIX is the doubled spelling the helpers exist to
    // prevent, named in their docs as the mistake - not a variable.
    found.retain(|name| !name.starts_with("DEGCT"));
    found
}

/// The names `env_for_child(X=.., Y=..)` sets, from the text after its opening bracket.
fn keywords(rest: &str) -> BTreeSet<String> {
    let until = rest.find(')').unwrap_or(rest.len());
    let inside = &rest[..until];
    let bytes = inside.as_bytes();
    let mut found = BTreeSet::new();
    let mut at = 0;
    while at < bytes.len() {
        let Some(name) = name_at(bytes, at) else {
            at += 1;
            continue;
        };
        let after = skip_spaces(bytes, at + name.len());
        if bytes.get(after) == Some(&b'=') && bytes.get(after + 1) != Some(&b'=') {
            found.insert(name.clone());
        }
        at += name.len().max(1);
    }
    found
}

/// Every index at which `needle` appears in `text`.
fn matches(text: &str, needle: &str) -> Vec<usize> {
    text.match_indices(needle).map(|(at, _)| at).collect()
}

/// The name starting at `from`: a letter, then letters, digits and underscores, ending in a
/// letter or a digit. `None` where what is there is not one.
fn name_at(bytes: &[u8], from: usize) -> Option<String> {
    if !bytes.get(from)?.is_ascii_uppercase() {
        return None;
    }
    let mut end = from;
    while end < bytes.len()
        && (bytes[end].is_ascii_uppercase() || bytes[end].is_ascii_digit() || bytes[end] == b'_')
    {
        end += 1;
    }
    while end > from && bytes[end - 1] == b'_' {
        end -= 1;
    }
    (end - from >= 2).then(|| String::from_utf8_lossy(&bytes[from..end]).into_owned())
}

fn skip_spaces(bytes: &[u8], mut at: usize) -> usize {
    while at < bytes.len() && (bytes[at] == b' ' || bytes[at] == b'\n' || bytes[at] == b'\t') {
        at += 1;
    }
    at
}

fn is_word(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

/// The rows in `these` and not in `those`, for a failure that says what to change.
fn only_in(these: &[String], those: &[String]) -> String {
    let those: BTreeSet<&String> = those.iter().collect();
    let missing: Vec<&str> = these
        .iter()
        .filter(|row| !those.contains(row))
        .map(String::as_str)
        .collect();
    if missing.is_empty() {
        "  (none)".to_string()
    } else {
        missing.join("\n")
    }
}
