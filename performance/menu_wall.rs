// SPDX-License-Identifier: MIT
//! What a WHOLE MENU costs, against the wall that now bounds one.
//!
//! ## The question
//!
//! `LookAheadTimeBudgetMs` bounds one option and a request is a whole menu, so what a player
//! waits for is the sum. de-dt75.3 asked whether that sum needs a wall of its own, and the
//! arithmetic that made it worth asking is this: a wide menu is a dozen options, a rolled
//! check is two searches rather than one, and the shipped per-option dial is a thousand
//! milliseconds - so the worst case a menu could name is around twenty-four seconds, under a
//! host read deadline of thirty whose response to being crossed is to KILL the engine.
//!
//! Whether that worst case is reachable is a measurement rather than an opinion, and this is
//! it. What it reports is the whole request through `Service::look_ahead` - the call the
//! engine host makes when the plugin sends a menu - so the number includes the graph, the
//! manager, the guards and the seed, and not only the searches.
//!
//! ## Adversarial on purpose, in three ways at once
//!
//! WIDE. `STARTS` defaults to twenty-four, which is the rolled-check case: twelve options of
//! which every one is a check. A menu that wide is rare and one where every option is a roll
//! does not occur, so this is the ceiling rather than a typical menu.
//!
//! DEEP. The starts come from `MenuProfile`, so exactly the structurally deepest entries are
//! unseen and every start has something better beyond it. That is what stops
//! `bridge::class_worth_hunting` refusing an option before a diagram is touched - the trap
//! that makes an easy profile read as a fast engine.
//!
//! COLD. Each conversation is asked ONCE, against a service that has not answered for that
//! group before, so every menu here pays the first-request warm-up that
//! `workspace_menus` measured at about twice a served request. A player's second menu in a
//! conversation is the cheaper one; this reports the first.
//!
//! ## How to run it
//!
//! ```text
//! tools/run-logged.sh cargo menu-wall -- \
//!   cargo run --release --example menu_wall
//! ```
//!
//! `--conversation 631,368` picks the groups, `--starts 12` the width, `--time-budget-ms` the
//! per-option dial and `--menu-time-budget-ms` the wall - which defaults to zero here, because
//! the question is what a menu costs WITHOUT one. Set it to see the wall bind.

use std::time::Instant;

use lookahead_engine::bridge::{LookAheadRequest, NodeRef, WorldRawData};
use lookahead_engine::core::types::DialogueNodeId;
use lookahead_engine::index::{build_group_graph, read_index};
use lookahead_engine::service::Service;

#[path = "../tests/common/mod.rs"]
mod common;

#[path = "options.rs"]
mod options;

#[path = "menu_profile.rs"]
mod menu_profile;
use menu_profile::MenuProfile;

/// The heaviest groups in the game, which is where a slow menu would live if one does.
const GROUPS: [i32; 6] = [362, 368, 631, 14, 28, 1030];

/// How many starts a menu asks about.
///
/// TWENTY-FOUR, which is the case de-dt75.3 was written about: twelve options, every one of
/// them a rolled check, so the engine runs two searches per option.
const STARTS: usize = 24;

/// How many of a group's deepest entries are unseen.
///
/// Ten, as every other whole-menu measurement uses, so a row here is comparable with one
/// there. Enough that a start's search has somewhere to go, few enough that it has to look.
const UNSEEN: usize = 10;

/// The shipped per-option dial, so the row is the one a player would get.
const TIME_BUDGET_MS: u64 = 1000;

/// The wall the plugin ships, reported beside each row rather than applied to it.
const MENU_WALL_MS: u64 = 3000;

/// The host's read deadline, which is not a budget: crossing it kills the engine.
const READ_DEADLINE_MS: u64 = 30000;

/// What this driver takes. With no group named it uses the list above.
#[derive(clap::Parser)]
#[command(about = "What a whole menu costs against a wall, group by group.")]
struct Options {
    #[command(flatten)]
    groups: options::Groups,
    #[command(flatten)]
    starts: options::Starts<STARTS>,
    /// What one option is allowed, in milliseconds
    #[arg(long = "time-budget-ms", value_name = "MS", default_value_t = TIME_BUDGET_MS)]
    time_budget_ms: u64,
    /// What the whole menu is allowed, in milliseconds; 0 is no wall
    #[arg(long = "menu-time-budget-ms", value_name = "MS", default_value_t = 0)]
    menu_time_budget_ms: u64,
}

fn main() {
    let asked = <Options as clap::Parser>::parse();
    let Some(path) = common::shipped_index() else {
        eprintln!("no shipped index; skipping.");
        return;
    };
    let index = read_index(&path).expect("the shipped index reads");

    let groups = asked.groups.or(&GROUPS);
    let starts_wanted = asked.starts.starts;
    let time_budget_ms = asked.time_budget_ms;
    let menu_budget_ms = asked.menu_time_budget_ms;

    println!(
        "a cold menu of up to {starts_wanted} starts per group, per-option budget \
         {time_budget_ms} ms, menu wall {}\n",
        if menu_budget_ms == 0 {
            "none".to_string()
        } else {
            format!("{menu_budget_ms} ms")
        },
    );
    println!(
        "{:>6}  {:>6}  {:>7}  {:>6}  {:>8}  {:>10}  {:>9}",
        "conv", "starts", "answers", "rolled", "menu ms", "slowest ms", "gave up",
    );

    let mut worst: Option<(i32, f64)> = None;
    for conversation in groups {
        let Ok((graph, _)) = build_group_graph(&index, conversation) else {
            continue;
        };
        let root = DialogueNodeId::new(conversation, 0);
        if graph.get(root).is_none() {
            continue;
        }
        let Some(profile) = MenuProfile::of(&graph, root, UNSEEN, starts_wanted) else {
            continue;
        };

        // WITH A WALK, as the product asks: built before the clock starts, since it stands in for
        // what the plugin records as a conversation plays - see `hub::walk_to_menu`.
        let request = LookAheadRequest {
            conversation,
            starts: profile.starts.iter().map(|id| NodeRef::from(*id)).collect(),
            encountered: lookahead_engine::symbolic::hub::walk_to_menu(
                &graph,
                conversation,
                &profile.starts,
            )
            .into_iter()
            .map(NodeRef::from)
            .collect(),
            // WHAT SOME PLAYTHROUGH SHOWED, which is everything the profile does not call
            // unseen anywhere - see `MenuProfile::seen_any_game`.
            seen_any_game: graph
                .nodes()
                .map(|node| node.id)
                .filter(|id| !profile.unseen.contains(id))
                .map(NodeRef::from)
                .collect(),
            time_budget_ms,
            menu_time_budget_ms: menu_budget_ms,
            world: WorldRawData {
                day_minutes: 720,
                day_counter: 1,
                ..Default::default()
            },
            ..Default::default()
        };

        // A SERVICE PER GROUP, so no group is answered by a workspace another group warmed.
        // Sharing one would make every row after the first cheaper for a reason that has
        // nothing to do with the menu it is reporting.
        let service = Service::open(&path, &common::declared_path()).expect("the engine opens");

        let began = Instant::now();
        let response = service.answer_request(request.clone());
        let took = began.elapsed().as_secs_f64() * 1000.0;

        let asked = request.starts.len();
        let answers = response.answers.len();
        let slowest = response
            .answers
            .iter()
            .map(|a| a.elapsed_ms)
            .max()
            .unwrap_or(0);
        let gave_up = response.answers.iter().filter(|a| !a.complete).count();

        println!(
            "{conversation:>6}  {asked:>6}  {answers:>7}  {:>6}  {took:>8.0}  {slowest:>10}  \
             {gave_up:>9}",
            answers.saturating_sub(asked),
        );

        if worst.is_none_or(|(_, ms)| took > ms) {
            worst = Some((conversation, took));
        }
    }

    let Some((group, ms)) = worst else {
        eprintln!("no group big enough; nothing measured.");
        return;
    };

    // THE TWO NUMBERS THE WORST MENU HAS TO BE READ AGAINST, and they are different in kind.
    // The wall is a budget: crossing it costs the options at the bottom of the menu their
    // markers. The read deadline is not: crossing that kills the engine.
    println!(
        "\nworst menu: conversation {group} at {ms:.0} ms - {:.1}x under the {MENU_WALL_MS} ms \
         wall, {:.0}x under the {READ_DEADLINE_MS} ms read deadline",
        MENU_WALL_MS as f64 / ms,
        READ_DEADLINE_MS as f64 / ms,
    );
}
