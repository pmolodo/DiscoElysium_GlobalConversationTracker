// SPDX-License-Identifier: MIT
//! Verifies that the mod's state file is the union of two or more saves.
//!
//! [`gct_state_check`] is the comparison and the report; this is the arguments, the files,
//! and the exit status.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gct_save_files::global_state::{self, GlobalState};
use gct_state_check::{DEFAULT_MAX_EXAMPLES, NamedSave};

/// What the mod's state file is called beside the saves.
const STATE_FILE: &str = "global-conversation-state.json";

/// Everything passed.
const PASS: u8 = 0;

/// An entry sits lower in the state than in a save.
const FAIL: u8 = 1;

/// Something could not be read, so the question was never asked.
const ERROR: u8 = 2;

const USAGE: &str = "\
gct-state-check - verify the mod's state file is the union of some saves.

Usage:
  gct-state-check <save> <save> [<save>...] [options]
  gct-state-check --list

Each <save> names a save: a path to a '<name>.ntwtf.zip', an expanded
'<name>.ntwtf' folder, or a bare save name resolved inside the SaveGames
directory. Two or more are required, because one proves nothing about a union.

Passes when, for every dialogue entry, the state file is at least as high as the
highest of the saves, ordering Untouched < WasOffered < WasDisplayed. A state
status strictly higher than every save is legal and is reported as information.

Options:
  -d, --dir PATH      SaveGames directory. Default: the game's own.
  -s, --state PATH    The state file. Default: <dir>/global-conversation-state.json
  -n, --examples N    How many entries to name per reported category.
      --list          List the saves in <dir> and exit.
  -h, --help          Show this message.

Exit codes: 0 pass, 1 fail, 2 usage or read error.";

/// What the arguments asked for.
struct Asked {
    saves: Vec<String>,
    directory: Option<PathBuf>,
    state: Option<PathBuf>,
    max_examples: usize,
    list: bool,
    help: bool,
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(why) => {
            eprintln!("{why}");
            ExitCode::from(ERROR)
        }
    }
}

fn run() -> Result<u8, String> {
    let asked = parse(&std::env::args().skip(1).collect::<Vec<_>>())?;
    if asked.help {
        println!("{USAGE}");
        return Ok(PASS);
    }

    let directory = asked.directory.unwrap_or_else(default_directory);
    if asked.list {
        println!("Saves in {}:", directory.display());
        for name in gct_state_check::saves_in(&directory) {
            println!("  {name}");
        }
        return Ok(PASS);
    }

    if asked.saves.len() < 2 {
        return Err(format!("At least two saves are required\n\n{USAGE}"));
    }

    let state_path = asked.state.unwrap_or_else(|| directory.join(STATE_FILE));
    println!("SaveGames directory : {}", directory.display());

    let mut saves = Vec::with_capacity(asked.saves.len());
    for named in &asked.saves {
        let path =
            gct_state_check::resolve_save(named, &directory).map_err(|why| why.to_string())?;
        let state = gct_state_check::statuses_in_save(&path).map_err(|why| why.to_string())?;

        println!("Save : {}", path.display());
        println!("       {}", described(&state));
        saves.push(NamedSave {
            label: path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            state,
        });
    }

    // NOT AN ASSERTION FAILURE AND NOT A PASS EITHER. With no file there is nothing to have
    // merged, so the run proves nothing and should not read as though it did.
    if !state_path.exists() {
        eprintln!("\nNo global state file at {}.", state_path.display());
        eprintln!(
            "The mod writes it on the first dialogue entry of a session, so this means no \
             tracked playthrough has happened yet."
        );
        return Ok(ERROR);
    }

    let shown = state_path.display().to_string();
    let text = std::fs::read_to_string(&state_path).map_err(|why| format!("{shown}: {why}"))?;
    let loaded = global_state::read(&text, &shown).map_err(|why| format!("{shown}: {why}"))?;

    println!("Global : {shown}");
    println!("          {}", described(&loaded.state));
    if !loaded.warnings.is_empty() {
        println!(
            "          {} unreadable row(s) skipped:",
            loaded.warnings.len()
        );
        for warning in &loaded.warnings {
            println!("            {warning}");
        }
    }
    println!();

    let report = gct_state_check::compare(&loaded.state, &saves).map_err(|why| why.to_string())?;
    print!("{}", gct_state_check::written(&report, asked.max_examples));

    Ok(if report.passed() { PASS } else { FAIL })
}

fn described(state: &GlobalState) -> String {
    format!(
        "{} entries above Untouched in {} conversations, and {} orbs",
        state.len(),
        state.conversations().count(),
        state.orbs().count(),
    )
}

/// Where the game keeps its saves, which is where this looks unless told otherwise.
fn default_directory() -> PathBuf {
    let home = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();

    Path::new(&home)
        .join("AppData")
        .join("LocalLow")
        .join("ZAUM Studio")
        .join("Disco Elysium")
        .join("SaveGames")
}

fn parse(argv: &[String]) -> Result<Asked, String> {
    let mut asked = Asked {
        saves: Vec::new(),
        directory: None,
        state: None,
        max_examples: DEFAULT_MAX_EXAMPLES,
        list: false,
        help: false,
    };

    let mut at = 0;
    while at < argv.len() {
        let argument = argv[at].as_str();
        let mut value = |what: &str| -> Result<String, String> {
            at += 1;
            argv.get(at)
                .cloned()
                .ok_or_else(|| format!("Option '{what}' requires a value"))
        };

        match argument {
            "-h" | "--help" => asked.help = true,
            "-d" | "--dir" => asked.directory = Some(PathBuf::from(value(argument)?)),
            "-s" | "--state" => asked.state = Some(PathBuf::from(value(argument)?)),
            "-n" | "--examples" => {
                let raw = value(argument)?;
                asked.max_examples = raw.parse().map_err(|_| {
                    format!(
                        "Option '{argument}' needs a whole number that is not negative, not '{raw}'"
                    )
                })?;
            }
            "--list" => asked.list = true,
            other if other.starts_with('-') => {
                return Err(format!("Unknown option '{other}'\n\n{USAGE}"));
            }
            other => asked.saves.push(other.to_string()),
        }

        at += 1;
    }

    Ok(asked)
}
