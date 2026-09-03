// SPDX-License-Identifier: MIT
//! The C ABI the game plugin reaches this engine through.
//!
//! The look-ahead used to live in C#, inside the BepInEx plugin, and is moving here - see
//! de-i5xj. The plugin still has to run it, from inside the game's Mono runtime, so it
//! comes back the ordinary way: this crate builds a `cdylib` and the plugin declares
//! `DllImport`s against it.
//!
//! ## The shape, and why it is this shape
//!
//! THE GRAPH DOES NOT CROSS. The engine builds it from `conversation_index.jsonl`, which
//! ships with the mod, so a caller names a conversation rather than marshalling four
//! thousand entries per menu. The index is read once, behind [`Engine`], and kept.
//!
//! THE WORLD CROSSES AS A SNAPSHOT, not as a callback. That is not a simplification, it
//! is what the C# already does: `GameLookAheadWorld` is built fresh per response menu and
//! caches each distinct query for the life of the crawl, because a menu asks the same
//! handful of questions hundreds of times as the search fans out. So the world a crawl
//! sees is a finite table of answers, and the set of questions is derivable from the
//! parsed guards rather than discovered by running. Passing the answers over means no
//! function pointers back into Mono, no reentrancy, and nothing that can deadlock.
//!
//! ## Rules this file lives by
//!
//! - NO PANIC MAY CROSS. Unwinding into Mono is undefined behaviour and would take the
//!   game down with a stack trace nobody can read. Every entry point wraps its work in
//!   [`catch_unwind`] and turns a panic into an error code.
//! - NO ALLOCATION CROSSES UNOWNED. A string this hands out is owned by this library and
//!   freed by [`gct_string_free`]; the caller never frees it itself, because the two sides
//!   have different allocators.
//! - EVERY POINTER IS CHECKED. A null or dangling handle is an error code, never a crash:
//!   the caller is a modded game, and the failure mode to avoid is one that looks like the
//!   game's own.

use std::ffi::{c_char, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use crate::index::{read_index, Index};

/// What an entry point returns. Zero is success; everything else is a reason.
pub const GCT_OK: c_int = 0;
/// A pointer argument was null, or a handle was not one this library handed out.
pub const GCT_BAD_HANDLE: c_int = -1;
/// A string argument was not valid UTF-8, or a path could not be read.
pub const GCT_BAD_ARGUMENT: c_int = -2;
/// The index could not be read.
pub const GCT_INDEX_UNREADABLE: c_int = -3;
/// Something panicked. The engine is still standing; the call did nothing.
pub const GCT_PANIC: c_int = -4;
/// No such conversation in the index.
pub const GCT_NO_SUCH_CONVERSATION: c_int = -5;

/// The engine, behind a handle the caller keeps.
///
/// Holds the index, which is tens of megabytes and takes a moment to parse. Read once when
/// the plugin loads and kept for the session; building it per response menu would put that
/// parse inside the frame that draws the menu.
pub struct Engine {
    index: Index,
}

/// Runs `work`, turning any panic into [`GCT_PANIC`].
///
/// Every entry point goes through here. `AssertUnwindSafe` is honest rather than
/// convenient: on a panic this returns a code and touches nothing the caller can observe,
/// so there is no half-updated state for a later call to trip over.
fn guarded<F: FnOnce() -> c_int>(work: F) -> c_int {
    catch_unwind(AssertUnwindSafe(work)).unwrap_or(GCT_PANIC)
}

/// Reads a C string argument, or `None` if it is null or not UTF-8.
///
/// # Safety
///
/// `text` must be null or a valid, NUL-terminated C string the caller keeps alive for the
/// length of the call.
unsafe fn borrowed(text: *const c_char) -> Option<&'static str> {
    if text.is_null() {
        return None;
    }

    unsafe { CStr::from_ptr(text) }.to_str().ok()
}

/// The engine behind a handle, or `None` if it is null.
///
/// # Safety
///
/// `handle` must be null or a pointer this library returned from
/// [`gct_engine_open`] and that has not been passed to [`gct_engine_close`].
unsafe fn engine<'a>(handle: *mut Engine) -> Option<&'a Engine> {
    if handle.is_null() {
        return None;
    }

    Some(unsafe { &*handle })
}

/// This library's version, as a NUL-terminated string the caller must not free.
///
/// Static, so it is exempt from the ownership rule above - there is nothing to free. It
/// exists so the plugin can check at load time that it has the library it was built
/// against, rather than discovering a mismatch through a wrong answer.
#[unsafe(no_mangle)]
pub extern "C" fn gct_version() -> *const c_char {
    // Built at compile time and NUL-terminated by hand, so this needs no allocation and
    // cannot fail.
    concat!(env!("CARGO_PKG_VERSION"), "\0").as_ptr() as *const c_char
}

/// Opens the engine over a conversation index, writing the handle to `out`.
///
/// # Safety
///
/// `index_path` must be a valid NUL-terminated UTF-8 path, and `out` a writable pointer to
/// one handle. On success the caller owns the handle and must pass it to
/// [`gct_engine_close`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_engine_open(
    index_path: *const c_char,
    out: *mut *mut Engine,
) -> c_int {
    guarded(|| {
        if out.is_null() {
            return GCT_BAD_ARGUMENT;
        }
        let Some(path) = (unsafe { borrowed(index_path) }) else {
            return GCT_BAD_ARGUMENT;
        };

        match read_index(&PathBuf::from(path)) {
            Ok(index) => {
                let engine = Box::new(Engine { index });
                unsafe { *out = Box::into_raw(engine) };
                GCT_OK
            }
            Err(_) => GCT_INDEX_UNREADABLE,
        }
    })
}

/// Closes an engine opened by [`gct_engine_open`].
///
/// Closing null is not an error, so a caller unwinding from a failed open does not have to
/// know how far it got.
///
/// # Safety
///
/// `handle` must be null, or a handle from [`gct_engine_open`] not already closed. Using it
/// afterwards is undefined.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_engine_close(handle: *mut Engine) -> c_int {
    guarded(|| {
        if !handle.is_null() {
            drop(unsafe { Box::from_raw(handle) });
        }
        GCT_OK
    })
}

/// How many conversations the index holds, written to `out`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_conversation_count(
    handle: *mut Engine,
    out: *mut c_int,
) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };

        unsafe { *out = engine.index.len() as c_int };
        GCT_OK
    })
}

/// How many entries one conversation holds, written to `out`.
///
/// The plugin's guard against a stale index. It builds its graph from the LIVE dialogue
/// database while this reads a file shipped with the mod, and if a game update or another
/// mod moves the content apart, the look-ahead would answer about a conversation the
/// player is not in. Comparing the entry count is the cheap half of noticing.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_entry_count(
    handle: *mut Engine,
    conversation: c_int,
    out: *mut c_int,
) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };

        match engine.index.get(&conversation) {
            Some(conversation) => {
                unsafe { *out = conversation.entries.len() as c_int };
                GCT_OK
            }
            None => GCT_NO_SUCH_CONVERSATION,
        }
    })
}

/// Frees a string this library handed out.
///
/// # Safety
///
/// `text` must be null or a string from this library that has not already been freed.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_string_free(text: *mut c_char) {
    if text.is_null() {
        return;
    }

    // The result is dropped; a panic here would be a bug in this file rather than
    // anything the caller did, and there is nothing useful to report it through.
    let _ = catch_unwind(AssertUnwindSafe(|| drop(unsafe { CString::from_raw(text) })));
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::ffi::CString;
    use std::ptr;

    /// The version is readable and is what Cargo says.
    #[test]
    fn the_version_comes_back_nul_terminated() {
        let version = unsafe { CStr::from_ptr(gct_version()) };
        assert_eq!(version.to_str().unwrap(), env!("CARGO_PKG_VERSION"));
    }

    /// Opening something that is not an index fails rather than panicking, which is the
    /// whole point of the error codes: the caller is a modded game.
    #[test]
    fn opening_a_path_that_is_not_an_index_reports_it() {
        let path = CString::new("no-such-file.jsonl").unwrap();
        let mut handle: *mut Engine = ptr::null_mut();
        let code = unsafe { gct_engine_open(path.as_ptr(), &mut handle) };

        assert_eq!(code, GCT_INDEX_UNREADABLE);
        assert!(handle.is_null(), "a failed open must not hand out a handle");
    }

    #[test]
    fn a_null_path_is_a_bad_argument_rather_than_a_crash() {
        let mut handle: *mut Engine = ptr::null_mut();
        assert_eq!(
            unsafe { gct_engine_open(ptr::null(), &mut handle) },
            GCT_BAD_ARGUMENT,
        );
    }

    #[test]
    fn a_null_handle_is_refused_by_every_reader() {
        let mut count: c_int = -99;
        assert_eq!(
            unsafe { gct_conversation_count(ptr::null_mut(), &mut count) },
            GCT_BAD_HANDLE,
        );
        assert_eq!(
            unsafe { gct_entry_count(ptr::null_mut(), 631, &mut count) },
            GCT_BAD_HANDLE,
        );
        assert_eq!(count, -99, "a refused call must not write to out");
    }

    /// Closing null is allowed, so a caller unwinding from a failed open need not know
    /// how far it got.
    #[test]
    fn closing_null_is_not_an_error() {
        assert_eq!(unsafe { gct_engine_close(ptr::null_mut()) }, GCT_OK);
    }

    #[test]
    fn freeing_null_is_not_an_error() {
        unsafe { gct_string_free(ptr::null_mut()) };
    }
}
