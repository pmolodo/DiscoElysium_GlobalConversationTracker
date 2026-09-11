// SPDX-License-Identifier: MIT
//! The look-ahead engine as a process the game talks to, rather than a library it loads.
//!
//! Reads framed requests from stdin and writes framed responses to stdout until the pipe
//! closes; [`lookahead_engine::host`] is the protocol and the reasoning behind it. There is
//! nothing else in here on purpose - a server with options is a server with a
//! configuration to get wrong, and everything this one needs arrives in the first request.
//!
//! ## Why this is not in `src/bin/`, where Cargo would find it by itself
//!
//! Because `src/.gitignore` ignores `bin/`, which every .NET project under `src/` needs it
//! to. A binary put in the place Cargo expects would be invisible to Git and would not
//! survive a fresh clone - so it lives here and `Cargo.toml` names it explicitly.
//!
//! ## Nothing but frames goes to stdout
//!
//! stdout is the wire. A stray `println!` anywhere in this process would be read by the
//! parent as a length and then as a body, and the stream would be out of step from that
//! point on - which is the failure that looks like a corrupt response rather than like a
//! stray print. Anything this process has to say goes to STDERR, which the parent can
//! capture and log without it meaning anything to the protocol.

//! ## The verbs, and why a server with none has any
//!
//! Run with no arguments this serves frames, exactly as it always has, and that mode still
//! has nothing to configure. Run with a VERB it is not a server at all - it does one thing
//! to one file and exits, and stdout carries the result rather than the wire.
//!
//! They are here rather than in a tool of their own because of what they do: they read and
//! write the formats this repository defines, which live in this crate, and C# reaches them
//! by running this binary. A second binary would be a second thing to find, deploy and keep
//! in step for no gain - and an FFI would be the cdylib `Cargo.toml` records the removal of.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lookahead_engine::formats::{json_diff, resolve};

/// What to do, where a verb was given.
enum Verb {
    /// Print one document with every diff between it and a whole document applied.
    Resolve { path: PathBuf },
    /// Write the diff that turns one document into another.
    Diff {
        base: PathBuf,
        target: PathBuf,
        out: PathBuf,
    },
}

fn main() -> ExitCode {
    let arguments: Vec<String> = std::env::args().skip(1).collect();
    if !arguments.is_empty() {
        return match verb(&arguments).and_then(perform) {
            Ok(()) => ExitCode::SUCCESS,
            Err(fault) => {
                let _ = writeln!(std::io::stderr(), "gct-engine-host: {fault}");
                ExitCode::FAILURE
            }
        };
    }

    let served = lookahead_engine::host::serve(std::io::stdin().lock(), std::io::stdout());

    match served {
        Ok(()) => ExitCode::SUCCESS,
        // The parent closing its end mid-frame, a frame past the limit, a write to a pipe
        // nobody is reading any more. None of these can be answered - the answer would go
        // down the same broken pipe - so they are said on stderr and the process ends.
        Err(fault) => {
            let _ = writeln!(std::io::stderr(), "gct-engine-host: {fault}");
            ExitCode::FAILURE
        }
    }
}

/// What the arguments ask for.
fn verb(arguments: &[String]) -> Result<Verb, String> {
    match arguments[0].as_str() {
        "resolve" if arguments.len() == 2 => Ok(Verb::Resolve {
            path: PathBuf::from(&arguments[1]),
        }),
        "diff" if arguments.len() == 4 => Ok(Verb::Diff {
            base: PathBuf::from(&arguments[1]),
            target: PathBuf::from(&arguments[2]),
            out: PathBuf::from(&arguments[3]),
        }),
        other => Err(format!(
            "'{other}' is not something this does. The verbs are:\n  \
             resolve <file>               print it with every diff beneath it applied\n  \
             diff <base> <target> <out>   write the diff that turns one into the other\n\
             With no arguments at all it serves the engine over stdin and stdout.",
        )),
    }
}

/// Does it, and says what happened where the answer is not the document itself.
fn perform(asked: Verb) -> Result<(), String> {
    match asked {
        Verb::Resolve { path } => {
            let document = resolve::document(&path).map_err(|fault| fault.to_string())?;
            println!(
                "{}",
                serde_json::to_string(&document).map_err(|fault| fault.to_string())?
            );
            Ok(())
        }
        Verb::Diff { base, target, out } => write_diff(&base, &target, &out),
    }
}

/// Writes the diff that turns one document into another, naming the first in it.
///
/// BOTH SIDES ARE RESOLVED FIRST, so a diff can be taken against a document that is itself
/// written as a diff, and what is compared is what a reader would get rather than what
/// happens to be on disk.
fn write_diff(base: &Path, target: &Path, out: &Path) -> Result<(), String> {
    let beneath = resolve::document(base).map_err(|fault| fault.to_string())?;
    let wanted = resolve::document(target).map_err(|fault| fault.to_string())?;

    let Some(patch) = json_diff::create(&beneath, &wanted) else {
        return Err(format!(
            "{} and {} are already the same document, so there is no diff to write.",
            base.display(),
            target.display(),
        ));
    };

    // THE BASE IS NAMED RELATIVE TO THE DIFF, because that is where a reader of the diff
    // stands when it follows the name.
    let named = relative(base, out.parent().unwrap_or_else(|| Path::new(".")));
    let mut patch = patch;
    patch.as_object_mut().expect("a diff is an object").insert(
        lookahead_engine::formats::header::BASE_KEY.to_string(),
        named.into(),
    );

    let text = serde_json::to_string_pretty(&patch).map_err(|fault| fault.to_string())?;
    std::fs::write(out, text + "\n").map_err(|fault| format!("{}: {fault}", out.display()))?;
    Ok(())
}

/// One path as it looks from a folder, where the two share one.
///
/// Enough for fixtures that sit beside each other, which is every one of them today; a base
/// somewhere else is named by whatever path the caller gave.
fn relative(path: &Path, from: &Path) -> String {
    match (path.parent(), path.file_name()) {
        (Some(folder), Some(name)) if folder == from => name.to_string_lossy().into_owned(),
        _ => path.to_string_lossy().into_owned(),
    }
}
