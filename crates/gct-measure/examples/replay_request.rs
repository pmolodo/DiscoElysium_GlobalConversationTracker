// SPDX-License-Identifier: MIT
//! A request the game sent, answered again offline.
//!
//! The plugin's `KeepLookAheadRequests` setting writes what crossed to the engine, and this
//! answers it through the same `Service` the engine host runs, so an in-game answer and an
//! offline one can be told apart by what they were ASKED rather than by where they ran. If the
//! game's own request is slow here too, the difference from an offline run is in the request;
//! if it is fast here, it is in how the game ran it.
//!
//! ## Two forms it reads
//!
//! THE GAME'S BYTES, `look-ahead-request-<conversation>.pb`: the message exactly as it crossed
//! to the engine, which the plugin writes beside the text rendering a person reads and the
//! harness keeps under `.build/automation/requests`. Read the way the engine host reads it,
//! through `wire_convert::read_look_ahead`.
//!
//! THE ENGINE'S SERDE FORM, any `.json`: what `--write-json` and `scenario_menus
//! --write-requests` write. A game request written out this way and an offline one are in the
//! same spelling, so they can be diffed - and edited one field at a time and answered again,
//! which is how a disagreement between the two is narrowed to the field that causes it.
//!
//! ```text
//! tools/run-logged.sh --kind analysis cargo replay-request -- \
//!   cargo run --release -p gct_measure --example replay_request -- \
//!   .build/automation/requests/look-ahead-request-631.pb --write-json game-631.json
//! ```

use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Instant;

use lookahead_engine::bridge::LookAheadRequest;
use lookahead_engine::service::Service;
use lookahead_engine::wire;
use lookahead_engine::wire_convert::read_look_ahead;
use prost::Message;

use gct_measure::common;

/// What this driver takes.
#[derive(clap::Parser)]
#[command(about = "A captured look-ahead request, answered again offline.")]
struct Options {
    /// The request: the game's .pb, or a .json in the engine's serde form
    request: PathBuf,
    /// Also write the decoded request here, in the engine's serde form
    #[arg(long = "write-json", value_name = "FILE")]
    write_json: Option<PathBuf>,
    /// Answer it this many times, one service each, to see how steady the cost is
    #[arg(long, default_value_t = 1)]
    runs: usize,
}

fn main() -> ExitCode {
    let asked = <Options as clap::Parser>::parse();
    match run(&asked) {
        Ok(()) => ExitCode::SUCCESS,
        Err(fault) => {
            eprintln!("replay_request: {fault}");
            ExitCode::FAILURE
        }
    }
}

fn run(asked: &Options) -> Result<(), String> {
    let bytes = std::fs::read(&asked.request)
        .map_err(|fault| format!("{}: {fault}", asked.request.display()))?;
    let request = if asked
        .request
        .extension()
        .is_some_and(|extension| extension == "json")
    {
        // THE MOD'S TEXT RENDERING IS JSON TOO, in protobuf's spelling rather than the
        // engine's, so it fails here - and the bytes that will read are right beside it.
        serde_json::from_slice::<LookAheadRequest>(&bytes).map_err(|fault| {
            format!(
                "{}: not a request in the engine's serde form: {fault}. A capture the mod \
                 wrote is read from the .pb beside it",
                asked.request.display(),
            )
        })?
    } else {
        let decoded = wire::LookAheadRequest::decode(bytes.as_slice())
            .map_err(|fault| format!("{}: not a request: {fault}", asked.request.display()))?;
        read_look_ahead(decoded).map_err(|fault| format!("the request will not read: {fault:?}"))?
    };

    if let Some(out) = &asked.write_json {
        let json = serde_json::to_string(&request)
            .map_err(|fault| format!("the request will not serialise: {fault}"))?;
        std::fs::write(out, json).map_err(|fault| format!("{}: {fault}", out.display()))?;
        println!("wrote {}", out.display());
    }

    let path = common::shipped_index().ok_or("there is no shipped index to answer from")?;
    println!(
        "conversation {}, {} starts, per-option budget {} ms, menu wall {} ms, memory {} MB",
        request.conversation,
        request.starts.len(),
        request.time_budget_ms,
        request.menu_time_budget_ms,
        request.memory_budget_mb,
    );

    for run in 1..=asked.runs {
        let service = Service::open(&path, &common::declared_path())
            .map_err(|status| format!("the engine will not open: {status:?}"))?;
        let began = Instant::now();
        let response = service.answer_request(request.clone());
        let took = began.elapsed().as_millis();
        if let Some(error) = response.error {
            return Err(error);
        }

        println!("\nrun {run}: the whole menu took {took} ms");
        println!(
            "  {:<10} {:<6} {:>4} {:<8} {:<7} {:>8} {:>13} {:>7}",
            "option", "branch", "best", "finished", "stopped", "ms", "diagram nodes", "reached",
        );
        for reply in &response.answers {
            println!(
                "  {:<10} {:<6} {:>4} {:<8} {:<7} {:>8} {:>13} {:>7}",
                format!("{}:{}", reply.start.conversation, reply.start.entry),
                reply.branch.as_deref().unwrap_or("-"),
                reply.best,
                if reply.complete { "yes" } else { "no" },
                reply.stopped_by,
                reply.elapsed_ms,
                reply.diagram_nodes,
                reply.nodes_reached,
            );
        }
    }
    Ok(())
}
