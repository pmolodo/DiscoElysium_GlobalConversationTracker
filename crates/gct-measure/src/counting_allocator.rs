// SPDX-License-Identifier: MIT
//! A global allocator that keeps a running total of what has been asked for.
//!
//! WHY AN ALLOCATOR RATHER THAN THE PROCESS'S RESIDENT SIZE: the question these callers ask
//! is how many bytes this library asks for, which is exactly what a global allocator sees.
//! Resident size answers a different question, and answers it late - the operating system
//! decides what to keep resident and when a page is first touched.
//!
//! ## The type is here, and the attribute is in the binary
//!
//! A `#[global_allocator]` is one per binary, and one declared in this crate would be imposed
//! on every binary that links it - every test and measurement would then be counting
//! allocations it has no interest in. So this only provides the type, and a caller that wants
//! the count installs it itself:
//!
//! ```no_run
//! use gct_measure::counting_allocator::{Counting, live};
//!
//! #[global_allocator]
//! static ALLOCATOR: Counting = Counting;
//!
//! fn main() {
//!     println!("{} bytes live", live());
//! }
//! ```
//!
//! ## One counter per process, and that is the point
//!
//! The total is the PROCESS'S, so anything else allocating at the same time is counted too.
//! `tests/manager_memory.rs` says what that cost once: two tests measuring against this
//! counter in parallel each read the other's allocations as their own and disagreed about
//! the same 6 GB manager by a factor of two, which looks exactly like a real finding and is
//! not one. Its rule - one test - still holds within a binary; the measurement beside it is
//! a separate process and so cannot interfere at all.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

/// An allocator that keeps a running total of what has been asked for.
pub struct Counting;

static LIVE: AtomicUsize = AtomicUsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc(layout) }
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(pointer, layout) }
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        unsafe { System.alloc_zeroed(layout) }
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        LIVE.fetch_add(new_size, Ordering::Relaxed);
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.realloc(pointer, layout, new_size) }
    }
}

/// What is currently allocated, in bytes.
pub fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}
