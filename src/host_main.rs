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

use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
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
