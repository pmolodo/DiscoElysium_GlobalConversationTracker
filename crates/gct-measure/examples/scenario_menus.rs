// SPDX-License-Identifier: MIT
//! A suite's scenarios played offline under the budgets the plugin ships, menu by menu.
//!
//! ## The question
//!
//! Whether the menus a scenario reaches are answered inside the budget, and what each option
//! cost. A suite that claims `everyMenuFinishesInBudget` has `tests/scenario_suites.rs` assert
//! the first half; this prints the second, for any suite, under any budgets.
//!
//! ## As the game asks it
//!
//! Both go through `common::staging::play_stops`, so what this prints is what the claim
//! checks: the save, the global state and the walk to each menu the in-game run stages, the
//! stops asked in order through one `Service` as the engine host asks them, and the budgets the
//! plugin ships - see `plugin_defaults`, held to `Plugin.cs` by a test. Each budget can be
//! overridden, to ask under a player's own config, and `--nolimit` lifts both time limits, to
//! ask how long a search that gave up would really need.
//!
//! ## What it cannot say
//!
//! How the game will time it. The deployed engine host runs in a process of its own, on other
//! threads, sharing the machine with the game, so an option that finishes near its wall here
//! can land on either side of it there. A search that takes several times its budget with
//! `--nolimit` is over budget anywhere.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo scenario-menus -- \
//!   cargo run --release -p gct_measure --example scenario_menus -- --suite joyce-wild-pines
//! ```

use std::process::ExitCode;

use lookahead_engine::bridge::{LookAheadAnswer, NodeRef};
use lookahead_engine::index::read_index;

use gct_measure::common;
use gct_measure::plugin_defaults::{self, Budgets};

use common::staging::{self, Staged};
use common::suites::{self, Scenario};

/// What this driver takes.
#[derive(clap::Parser)]
#[command(about = "A suite's scenarios played offline under the shipped look-ahead budgets.")]
struct Options {
    /// The suite in testing/scenarios/suites.json whose scenarios to play
    #[arg(long, value_name = "NAME")]
    suite: String,
    /// Only this scenario: a save, or a save and a conversation as SAVE:CONVERSATION
    #[arg(long, value_name = "SAVE[:CONVERSATION]")]
    scenario: Option<String>,
    /// What one option is allowed, in milliseconds; 0 is no limit
    #[arg(long = "time-budget-ms", value_name = "MS", default_value_t = plugin_defaults::TIME_BUDGET_MS)]
    time_budget_ms: u64,
    /// What the whole menu is allowed, in milliseconds; 0 is no limit
    #[arg(long = "menu-time-budget-ms", value_name = "MS", default_value_t = plugin_defaults::MENU_TIME_BUDGET_MS)]
    menu_time_budget_ms: u64,
    /// What one option's search may hold, in megabytes
    #[arg(long = "memory-budget-mb", value_name = "MB", default_value_t = plugin_defaults::MEMORY_BUDGET_MB)]
    memory_budget_mb: usize,
    /// Lift both time limits, to see what a search that gives up would really need
    #[arg(long, conflicts_with_all = ["time_budget_ms", "menu_time_budget_ms"])]
    nolimit: bool,
}

impl Options {
    /// Whether `--scenario` takes this one.
    fn takes(&self, scenario: &Scenario) -> bool {
        let Some(wanted) = &self.scenario else {
            return true;
        };
        match wanted.split_once(':') {
            Some((save, conversation)) => {
                save == scenario.save && conversation == scenario.conversation.to_string()
            }
            None => wanted == &scenario.save,
        }
    }

    /// The budgets asked for, where a time limit of zero is none.
    fn budgets(&self) -> Budgets {
        let limit = |milliseconds| if self.nolimit { 0 } else { milliseconds };
        Budgets {
            time_budget_ms: limit(self.time_budget_ms),
            menu_time_budget_ms: limit(self.menu_time_budget_ms),
            memory_budget_mb: self.memory_budget_mb,
        }
    }
}

/// A time limit as a person reads it.
fn shown(milliseconds: u64) -> String {
    if milliseconds == 0 {
        "none".to_string()
    } else {
        format!("{milliseconds} ms")
    }
}

/// An entry as the overflow log and the suite rows name it.
fn named(node: NodeRef) -> String {
    format!("{}:{}", node.conversation, node.entry)
}

fn main() -> ExitCode {
    let asked = <Options as clap::Parser>::parse();
    match run(&asked) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("scenario_menus: {fault}");
            ExitCode::FAILURE
        }
    }
}

fn run(asked: &Options) -> Result<(), String> {
    let path = common::shipped_index().ok_or("there is no shipped index to answer from")?;
    let index = read_index(&path).map_err(|fault| format!("{}: {fault}", path.display()))?;
    // THE FULL INDEX AS WELL, for the one thing the shipped one cannot answer: it carries no
    // dialogue text, so whether a walked line has any can only be read there.
    let texts_path = common::conversation_index().ok_or("there is no full conversation index")?;
    let texts =
        read_index(&texts_path).map_err(|fault| format!("{}: {fault}", texts_path.display()))?;

    let table = suites::table();
    let suite = table.suite(&asked.suite);
    let scenarios: Vec<&Scenario> = suite
        .scenarios
        .iter()
        .filter(|scenario| asked.takes(scenario))
        .collect();
    if scenarios.is_empty() {
        return Err(format!(
            "suite '{}' has no scenario matching '{}'",
            suite.suite,
            asked.scenario.as_deref().unwrap_or_default(),
        ));
    }

    let budgets = asked.budgets();
    println!(
        "suite {}, global state {}: per-option budget {}, menu wall {}, memory {} MB",
        suite.suite,
        suite.state,
        shown(budgets.time_budget_ms),
        shown(budgets.menu_time_budget_ms),
        budgets.memory_budget_mb,
    );

    for scenario in scenarios {
        let (staged, played) =
            staging::play_stops(&index, &texts, &path, suite, scenario, budgets)?;
        for stop in played {
            if !stop.silent.is_empty() {
                eprintln!(
                    "warning: the walk displays {:?}, which have no text, and whether the game \
                     waits on such a line is unmeasured - this may not be the menu the game draws",
                    stop.silent,
                );
            }

            println!(
                "\n{} {}, {}\n  menu {}",
                scenario.save,
                scenario.conversation,
                stop.what,
                stop.walk
                    .menu
                    .iter()
                    .map(|id| named(NodeRef::from(*id)))
                    .collect::<Vec<_>>()
                    .join(" "),
            );
            report(&staged, &stop.response.answers);

            let answers = &stop.response.answers;
            let finished = answers.iter().filter(|reply| reply.complete).count();
            println!(
                "  {finished} of {} answers finished; the whole menu took {} ms",
                answers.len(),
                stop.took_ms,
            );
        }
    }
    Ok(())
}

/// One row per answer, in the order the engine gave them.
fn report(staged: &Staged, answers: &[LookAheadAnswer]) {
    println!(
        "  {:<10} {:<6} {:>3} {:>4} {:<7} {:<8} {:<7} {:>8} {:>13} {:>7}  witness",
        "option",
        "branch",
        "own",
        "best",
        "drawn",
        "finished",
        "stopped",
        "ms",
        "diagram nodes",
        "reached",
    );
    for reply in answers {
        let own = staged.seen_state_of(reply.start);
        println!(
            "  {:<10} {:<6} {own:>3} {:>4} {:<7} {:<8} {:<7} {:>8} {:>13} {:>7}  {}",
            named(reply.start),
            reply.branch.as_deref().unwrap_or("-"),
            reply.best,
            staging::drawn(own, reply),
            if reply.complete { "yes" } else { "no" },
            reply.stopped_by,
            reply.elapsed_ms,
            reply.diagram_nodes,
            reply.nodes_reached,
            reply.witness.map_or("-".to_string(), named),
        );
    }
}
