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
//!
//! ## A verb may print something the caller cannot work out for itself
//!
//! `pack` prints the archive it wrote. The name asked for is not always the name written -
//! a save's name carries a timestamp and one without is given it - so the caller reads the
//! answer off stdout rather than assuming the path it passed in.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use lookahead_engine::formats::expanded_save::{self, OnDisk};
use lookahead_engine::formats::lua_blob::{self, LuaValue, TABLE_NAMES};
use lookahead_engine::formats::lua_simx::{self, Orders};
use lookahead_engine::formats::packed_save::{self, Stamp};
use lookahead_engine::formats::{
    convert, expand, json_diff, lua_parts, lua_sparse, resolve, sparse,
};

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
    /// Write an expanded save as the archive the game loads.
    Pack { source: PathBuf, out: PathBuf },
    /// Write the archive the game wrote as an expanded save.
    Expand {
        source: PathBuf,
        out: PathBuf,
        base: Option<PathBuf>,
    },
    /// Write a committed save again, as this build would write it.
    Rewrite { save: PathBuf },
    /// Bring a file up to the current version of its own format.
    Convert {
        input: PathBuf,
        out: Option<PathBuf>,
    },
    /// Print one of a save's Lua tables, as the tree it is stored as.
    Dump { save: PathBuf, table: String },
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
        "pack" if arguments.len() == 3 => Ok(Verb::Pack {
            source: PathBuf::from(&arguments[1]),
            out: PathBuf::from(&arguments[2]),
        }),
        "expand" if matches!(arguments.len(), 3 | 4) => Ok(Verb::Expand {
            source: PathBuf::from(&arguments[1]),
            out: PathBuf::from(&arguments[2]),
            base: arguments.get(3).map(PathBuf::from),
        }),
        "rewrite" if arguments.len() == 2 => Ok(Verb::Rewrite {
            save: PathBuf::from(&arguments[1]),
        }),
        "convert" if matches!(arguments.len(), 2 | 3) => Ok(Verb::Convert {
            input: PathBuf::from(&arguments[1]),
            out: arguments.get(2).map(PathBuf::from),
        }),
        "dump" if matches!(arguments.len(), 2 | 3) => Ok(Verb::Dump {
            save: PathBuf::from(&arguments[1]),
            table: arguments
                .get(2)
                .cloned()
                .unwrap_or_else(|| lua_sparse::CONVERSATION_TABLE.to_string()),
        }),
        other => Err(format!(
            "'{other}' is not something this does. The verbs are:\n  \
             resolve <file>                     print it with every diff beneath it applied\n  \
             diff <base> <target> <out>         write the diff that turns one into the other\n  \
             pack <save.ntwtf> <out.zip>        write an expanded save as the game's archive\n  \
             expand <save.zip> <out> [<base>]   write the game's archive as an expanded save,\n  \
             \x20                               as a change to <base> where one is named\n  \
             rewrite <save.ntwtf>               write a committed save again, as this build\n  \
             \x20                               writes one, in place\n  \
             convert <file> [<out>]             bring a file up to the current version of\n  \
             \x20                               its own format; in place where <out> is\n  \
             \x20                               not named, keeping the original beside it\n  \
             dump <save> [<table>]              print one of a save's five Lua tables;\n  \
             \x20                               the conversations where none is named\n\
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
        Verb::Pack { source, out } => {
            // WHAT IT WROTE, which is not always what it was asked for: a name carrying no
            // timestamp is given one, and the caller has no way to work out which.
            let written =
                packed_save::pack(&OnDisk, &source, &out, orders().as_ref(), Stamp::now())
                    .map_err(|fault| fault.to_string())?;
            println!("{}", written.display());
            Ok(())
        }
        Verb::Expand { source, out, base } => {
            let packed = packed_save::unpack(&source).map_err(|fault| fault.to_string())?;
            let files =
                expand::expansion(&OnDisk, &packed, &out, base.as_deref(), orders().as_ref())
                    .map_err(|fault| fault.to_string())?;
            expanded_save::write_all(&out, &files).map_err(|fault| fault.to_string())?;
            Ok(())
        }
        Verb::Rewrite { save } => {
            // PLANNED IN FULL BEFORE ANYTHING IS REMOVED. What is on disk is what the plan
            // is read from, so it cannot be taken away until there is a whole plan to put
            // in its place.
            let files = expand::rewrite(&OnDisk, &save, orders().as_ref())
                .map_err(|fault| fault.to_string())?;

            std::fs::remove_dir_all(&save)
                .map_err(|fault| format!("{}: {fault}", save.display()))?;
            expanded_save::write_all(&save, &files).map_err(|fault| fault.to_string())?;

            println!("{}", save.display());
            Ok(())
        }
        Verb::Convert { input, out } => convert_file(&input, out.as_deref()),
        Verb::Dump { save, table } => dump_table(&save, &table),
    }
}

/// Prints one of a save's five Lua tables, as the sparse tree a split directory holds.
///
/// THE SPARSE FORM RATHER THAN THE BLOB'S OWN LAYOUT, which is what a person reading a save
/// wants: it keeps every key and every value and drops where the list half ended, which is
/// the game's business and never a question anybody is asking here.
///
/// # Errors
///
/// Where the save will not read, or names a table that is not one of the five.
fn dump_table(save: &Path, table: &str) -> Result<(), String> {
    let shown = save.display().to_string();
    let at = TABLE_NAMES
        .iter()
        .position(|name| name.eq_ignore_ascii_case(table))
        .ok_or_else(|| {
            format!(
                "'{table}' is not one of a save's tables. They are: {}",
                TABLE_NAMES.join(", ")
            )
        })?;

    // WHICHEVER SHAPE THE SAVE IS IN. An expanded one already holds the trees; a packed one
    // holds the blob, and encoding it is how the two come out looking the same.
    let tree = if save.is_dir() {
        lua_parts::read(&OnDisk, save)
            .map_err(|fault| format!("{shown}: {fault}"))?
            .tables
            .swap_remove(at)
    } else {
        let packed = packed_save::unpack(save).map_err(|fault| format!("{shown}: {fault}"))?;
        let blob = lua_blob::read(&packed.lua).map_err(|fault| format!("{shown}: {fault}"))?;
        let LuaValue::Table(held) = &blob.tables[at] else {
            return Err(format!("{shown} holds no {} table", TABLE_NAMES[at]));
        };

        lua_sparse::encode(held, TABLE_NAMES[at], None)
            .map_err(|fault| format!("{shown}: {fault}"))?
    };

    print!("{}", sparse::write(&tree));
    Ok(())
}

/// Brings one file up to the current version of its own format.
///
/// IN PLACE WHERE NO OUTPUT IS NAMED, which is what makes converting a file mean the file
/// is then readable: a reader looks for a file by its name, so a conversion written under
/// another name leaves the reader facing the same old file. The original is kept beside it.
///
/// # Errors
///
/// Where the file will not read, is not a format this build knows, was written by a newer
/// build, or where the output - or, in place, the name the original would be kept under -
/// is already there.
fn convert_file(input: &Path, out: Option<&Path>) -> Result<(), String> {
    let shown = input.display().to_string();
    let bytes = std::fs::read(input).map_err(|fault| format!("{shown}: {fault}"))?;
    let what = convert::detect(&bytes, &shown).map_err(|fault| fault.to_string())?;

    if what.is_current() {
        println!(
            "{shown} is already {} version {}, which is what this build writes. Nothing to do.",
            what.name, what.version,
        );
        return Ok(());
    }

    // REFUSED, NOT CONVERTED, and this is the direction that matters most. A file from a
    // newer build is full of real history this one cannot read all of, so writing anything
    // at all would be inventing the parts it could not.
    if what.is_from_the_future() {
        return Err(format!(
            "{shown} is {} version {}, and this build writes version {} at most. It was \
             written by a newer build and is not damaged, so do not overwrite it - use a \
             build at least as new as the one that wrote it.",
            what.name, what.version, what.current,
        ));
    }

    // CONVERTED WHOLE BEFORE ANYTHING ON DISK IS TOUCHED, so a file that will not convert
    // leaves the input exactly as it was, whichever way the output was asked for.
    let converted =
        convert::to_current(&what, &bytes, &shown).map_err(|fault| fault.to_string())?;

    let Some(out) = out else {
        return convert_in_place(input, &what, &converted);
    };
    if out == input {
        return Err(
            "the input and the output must be different files; name no output to convert \
             the file in place"
                .to_string(),
        );
    }

    // REFUSED WHERE THE OUTPUT IS ALREADY THERE rather than replaced. What is being
    // converted is somebody's history, and the output may be an earlier conversion of it.
    if out.exists() {
        return Err(format!("{} is already there", out.display()));
    }
    std::fs::write(out, converted).map_err(|fault| format!("{}: {fault}", out.display()))?;

    println!(
        "Converted {shown} from {} version {} to version {} at {}.",
        what.name,
        what.version,
        what.current,
        out.display(),
    );
    Ok(())
}

/// Puts the converted bytes where the original was, and keeps the original beside them
/// under the version it was.
///
/// ## The order, and why
///
/// The converted bytes are written to a file of their own first, then the original is moved
/// to the name it is kept under, then the converted file is moved into the original's name.
/// A stop at any point leaves the original intact under one name or the other, and never
/// half a file where the original was.
///
/// NOTHING IS REPLACED. The name the original is kept under, and the file the conversion is
/// written to first, are each refused where something is already there: either may be an
/// earlier conversion, or somebody's own file.
///
/// # Errors
///
/// Where either of those names is taken, or a write or a move fails. A failed final move
/// puts the original back where it was.
fn convert_in_place(
    input: &Path,
    what: &convert::Detected,
    converted: &[u8],
) -> Result<(), String> {
    let shown = input.display();
    let kept = convert::beside(input, what.version);
    if kept.exists() {
        return Err(format!(
            "{} is already there, and it is where the original would be kept. Move it aside, \
             or name an output to convert to instead.",
            kept.display(),
        ));
    }

    let mut pending_name = input.file_name().unwrap_or_default().to_os_string();
    pending_name.push(CONVERTING_SUFFIX);
    let pending = input.with_file_name(pending_name);
    if pending.exists() {
        return Err(format!(
            "{} is already there, and it is where the conversion is written before it replaces \
             the original. It may be left from a conversion that stopped part way; look at it, \
             then move it aside.",
            pending.display(),
        ));
    }

    std::fs::write(&pending, converted)
        .map_err(|fault| format!("{}: {fault}", pending.display()))?;

    if let Err(fault) = std::fs::rename(input, &kept) {
        let _ = std::fs::remove_file(&pending);
        return Err(format!(
            "could not move {shown} to {}: {fault}. Nothing was changed.",
            kept.display()
        ));
    }

    if let Err(fault) = std::fs::rename(&pending, input) {
        return Err(match std::fs::rename(&kept, input) {
            Ok(()) => format!(
                "could not move the conversion into {shown}: {fault}. The original is back \
                 where it was, and the conversion is at {}.",
                pending.display(),
            ),
            Err(back) => format!(
                "could not move the conversion into {shown}: {fault}, nor the original back: \
                 {back}. The original is at {} and the conversion at {}.",
                kept.display(),
                pending.display(),
            ),
        });
    }

    println!(
        "Converted {shown} from {} version {} to version {}, in place. The original is kept at \
         {}.",
        what.name,
        what.version,
        what.current,
        kept.display(),
    );
    Ok(())
}

/// What the file a conversion is written to first is called: the input's own name with this
/// after it.
const CONVERTING_SUFFIX: &str = ".converting";

/// The id map a save's derived variables are rebuilt from, where it is to be found.
///
/// NOT AN ERROR WHEN IT IS NOT THERE, because only the `Variable` table needs it and
/// [`packed_save`] refuses a save that needs one without it, naming the file. Turning a
/// missing map into a failure here would refuse saves that do not need it and would say so
/// before knowing whether this one did.
fn orders() -> Option<Orders> {
    let text = std::fs::read_to_string(beside_us(lua_simx::ORDERS_FILE_NAME)?).ok()?;
    Orders::read(&text).ok()
}

/// A repository file, looked for from the working directory and from this binary outwards.
///
/// TWO STARTING POINTS because the two are different places and either can be the one
/// inside the checkout: the host is run from the repository root by a person, and from
/// wherever the game is by the harness and the plugin.
fn beside_us(name: &str) -> Option<PathBuf> {
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|path| path.parent().map(Path::to_path_buf)),
    ];

    for start in starts.into_iter().flatten() {
        for folder in start.ancestors() {
            let candidate = folder.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }

    None
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
    let named = expanded_save::relative(out.parent().unwrap_or_else(|| Path::new(".")), base);
    let mut patch = patch;
    patch.as_object_mut().expect("a diff is an object").insert(
        lookahead_engine::formats::header::BASE_KEY.to_string(),
        named.into(),
    );

    let text = serde_json::to_string_pretty(&patch).map_err(|fault| fault.to_string())?;
    std::fs::write(out, text + "\n").map_err(|fault| format!("{}: {fault}", out.display()))?;
    Ok(())
}
