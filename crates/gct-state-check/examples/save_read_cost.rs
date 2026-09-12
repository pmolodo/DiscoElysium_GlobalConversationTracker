// SPDX-License-Identifier: MIT
//! What reading a real save's dialogue statuses costs, in time and in memory held.
//!
//! ## The question this answers
//!
//! The plugin reads a save's statuses in C#, with a STREAMING visitor that never builds the
//! whole table. Moving that into the library the mod links would mean building it - Rust's
//! blob reader returns the tables rather than walking past them - and a save's blob is tens
//! of megabytes. Whether that is affordable ON A SAVE LOAD, while a player is waiting, is a
//! thing to measure rather than to hope. See de-xz48.6.9.
//!
//! ## What it reports
//!
//! Time for each step, and the PEAK BYTES HELD, which is the number that decides it. Held
//! rather than allocated: what matters is how much is live at once, since that is what the
//! game's heap has to find.
//!
//! Run it over a save the game actually wrote:
//!
//! ```text
//! cargo run --release -p gct_state_check --example save_read_cost -- <save.ntwtf.zip>
//! ```

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

/// What is live right now, and the most that has ever been.
static HELD: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, counting what it hands out and what comes back.
struct Counted;

unsafe impl GlobalAlloc for Counted {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's contract, passed straight through.
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            let now = HELD.fetch_add(layout.size(), Ordering::Relaxed) + layout.size();
            PEAK.fetch_max(now, Ordering::Relaxed);
        }

        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        HELD.fetch_sub(layout.size(), Ordering::Relaxed);
        // SAFETY: the caller's contract, passed straight through.
        unsafe { System.dealloc(pointer, layout) };
    }
}

#[global_allocator]
static ALLOCATOR: Counted = Counted;

/// A byte count as a person reads it.
fn megabytes(bytes: usize) -> String {
    #[allow(clippy::cast_precision_loss)]
    let mb = bytes as f64 / (1024.0 * 1024.0);
    format!("{mb:.1} MB")
}

fn main() {
    let Some(named) = std::env::args().nth(1) else {
        eprintln!("usage: save_read_cost <save.ntwtf.zip>");
        std::process::exit(2);
    };

    let path = std::path::PathBuf::from(named);
    println!("save            : {}", path.display());

    let began = Instant::now();
    let packed = gct_formats::packed_save::unpack(&path).expect("it unpacks");
    println!("unpack          : {:?}", began.elapsed());
    println!("blob            : {}", megabytes(packed.lua.len()));

    let before = HELD.load(Ordering::Relaxed);
    PEAK.store(before, Ordering::Relaxed);

    let began = Instant::now();
    let blob = gct_formats::lua_blob::read(&packed.lua).expect("it reads");
    let read = began.elapsed();
    let held = HELD.load(Ordering::Relaxed) - before;

    println!("lua_blob::read  : {read:?}");
    println!("  held after    : {}", megabytes(held));
    println!(
        "  peak during   : {}",
        megabytes(PEAK.load(Ordering::Relaxed) - before)
    );
    drop(blob);

    let began = Instant::now();
    let statuses = gct_state_check::statuses_in_save(&path).expect("it projects");
    println!("whole path      : {:?}", began.elapsed());
    println!("entries         : {}", statuses.len());
    println!(
        "peak overall    : {}",
        megabytes(PEAK.load(Ordering::Relaxed)),
    );
}
