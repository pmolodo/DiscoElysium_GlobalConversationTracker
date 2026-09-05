// SPDX-License-Identifier: MIT
//! The C ABI the game plugin reaches this engine through.
//!
//! The look-ahead used to live in C#, inside the BepInEx plugin, and is moving here - see
//! de-i5xj. The plugin still has to run it, from inside the game process, so it comes back
//! the ordinary way: this crate builds a `cdylib` and the plugin declares `DllImport`s
//! against it. BepInEx 6.0.0-be.688 runs IL2CPP plugins on CoreCLR .NET 6, so that is an
//! ordinary P/Invoke rather than anything exotic.
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
//! function pointers back into managed code, no reentrancy, and nothing that can deadlock.
//!
//! ## Rules this file lives by
//!
//! - NO PANIC MAY CROSS. Unwinding into managed frames is undefined behaviour, and would
//!   take the game down with a stack trace nobody can read. Every entry point wraps its
//!   work in [`catch_unwind`] and turns a panic into an error code.
//! - NO ALLOCATION CROSSES UNOWNED. A string this hands out is owned by this library and
//!   freed by [`gct_string_free`]; the caller never frees it itself, because the two sides
//!   have different allocators.
//! - EVERY POINTER IS CHECKED. A null or dangling handle is an error code, never a crash:
//!   the caller is a modded game, and the failure mode to avoid is one that looks like the
//!   game's own.

use std::ffi::{c_char, c_int, CStr, CString};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::path::PathBuf;

use crate::service::{Service, Status};

// THE NUMBERS COME FROM [`Status`] RATHER THAN BEING RESTATED HERE. There are two front
// ends over the same work now - this one and the pipe in [`crate::host`] - and a code that
// meant one thing over the ABI and another over the pipe would be the worst kind of
// mismatch, since both sides would look correct in isolation. The .NET `Status` enum is
// kept in step with these by hand, which is one place rather than two.
/// What an entry point returns. Zero is success; everything else is a reason.
pub const GCT_OK: c_int = Status::Ok as c_int;
/// A pointer argument was null, or a handle was not one this library handed out.
pub const GCT_BAD_HANDLE: c_int = Status::BadHandle as c_int;
/// A string argument was not valid UTF-8, or a path could not be read.
pub const GCT_BAD_ARGUMENT: c_int = Status::BadArgument as c_int;
/// The index could not be read.
pub const GCT_INDEX_UNREADABLE: c_int = Status::IndexUnreadable as c_int;
/// Something panicked. The engine is still standing; the call did nothing.
pub const GCT_PANIC: c_int = Status::Panic as c_int;
/// No such conversation in the index.
pub const GCT_NO_SUCH_CONVERSATION: c_int = Status::NoSuchConversation as c_int;
/// An answer could not be turned into JSON. Should not happen; reported anyway.
pub const GCT_SERIALISE_FAILED: c_int = Status::SerialiseFailed as c_int;

/// The engine, behind a handle the caller keeps.
///
/// A [`Service`] and the C ownership rules around it, and nothing else: the work moved out
/// so that a second front end could reach it without going through a DLL. What is left
/// here is the pointer checking, the panic catching and the string ownership - the three
/// things that are about the ABI rather than about the engine.
pub struct Engine(Service);

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
/// `variables_path` is the database's variable table, or null. Passed rather than looked
/// for beside the index because the caller renames what it deploys: only
/// `GlobalConversationTracker*` is installed next to the plugin, so a library hunting for
/// `variables.jsonl` would never find the file that is actually there. Null, or a path that
/// will not read, is not an error - the mod works without it, one variable in seventy-five
/// answering less precisely. [`gct_variable_count`] says which happened.
///
/// # Safety
///
/// `index_path` must be a valid NUL-terminated UTF-8 path, `variables_path` null or the
/// same, and `out` a writable pointer to one handle. On success the caller owns the handle
/// and must pass it to [`gct_engine_close`].
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_engine_open(
    index_path: *const c_char,
    variables_path: *const c_char,
    out: *mut *mut Engine,
) -> c_int {
    guarded(|| {
        if out.is_null() {
            return GCT_BAD_ARGUMENT;
        }
        let Some(path) = (unsafe { borrowed(index_path) }) else {
            return GCT_BAD_ARGUMENT;
        };
        let variables = unsafe { borrowed(variables_path) }.map(PathBuf::from);

        match Service::open(&PathBuf::from(path), variables.as_deref()) {
            Ok(service) => {
                unsafe { *out = Box::into_raw(Box::new(Engine(service))) };
                GCT_OK
            }
            Err(status) => status as c_int,
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

        unsafe { *out = engine.0.conversation_count() };
        GCT_OK
    })
}

/// How many variables the deployed table declares, written to `out`; zero if none was.
///
/// The plugin logs this at load for the same reason it logs the conversation count: a table
/// that was not deployed, or that would not read, is a mod that still works and answers one
/// variable in seventy-five less precisely - which is exactly the kind of thing that is
/// never noticed unless a line says it.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_variable_count(handle: *mut Engine, out: *mut c_int) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };

        unsafe { *out = engine.0.variable_count() };
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

        match engine.0.entry_count(conversation) {
            Ok(count) => {
                unsafe { *out = count };
                GCT_OK
            }
            Err(status) => status as c_int,
        }
    })
}

/// What one conversation's content reduced to when the index was written.
///
/// Writes the stored hash to `out` as JSON-free text the caller must free with
/// [`gct_string_free`], or an EMPTY string where the index carries none - which is what
/// the full index looks like, and means "this cannot be validated" rather than "this is
/// wrong".
///
/// THIS ENGINE NEVER HASHES. The extractor computes it from 170 MB of YAML and the plugin
/// computes it from the live dialogue database, through one shared routine; a third writer
/// here would be a third thing to keep in step, over a third representation, for no gain.
/// All this does is hand back what it was given.
///
/// # Safety
///
/// `handle` must be an open engine and `out` a writable pointer to one string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_conversation_hash(
    handle: *mut Engine,
    conversation: c_int,
    out: *mut *mut c_char,
) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };

        match engine.0.conversation_hash(conversation) {
            Ok(hash) => write_text(hash, out),
            Err(status) => status as c_int,
        }
    })
}

/// What version the opened index says it is, written to `out`; 0 where it has no header.
///
/// Zero is not a failure. The full index has no header, carries no hashes, and is a build
/// intermediate rather than a cache - a mod shipping one works exactly as it did before
/// there was such a thing as validation, and simply cannot check itself.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_index_format(handle: *mut Engine, out: *mut c_int) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };

        unsafe { *out = engine.0.index_format() };
        GCT_OK
    })
}

/// Every question a crawl over one conversation's group can ask the world.
///
/// Writes JSON to `out`, which the caller must free with [`gct_string_free`]. The plugin
/// answers these keys and hands them back in a look-ahead request; see
/// [`crate::bridge::Questions`] for why the engine names its own keys rather than letting
/// the caller build them.
///
/// # Safety
///
/// `handle` must be an open engine and `out` a writable pointer to one string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_questions(
    handle: *mut Engine,
    conversation: c_int,
    out: *mut *mut c_char,
) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };

        match engine.0.questions(conversation) {
            Ok(questions) => write_json(&questions, out),
            Err(status) => status as c_int,
        }
    })
}

/// Answers a look-ahead request.
///
/// `request` is JSON - see [`crate::bridge::LookAheadRequest`] - and the answer is written
/// to `out` as JSON the caller must free with [`gct_string_free`].
///
/// A request that cannot be served at all comes back as a response carrying `error`
/// rather than as a status code, so the caller has one thing to parse and one place to
/// look. The status codes are for the things that happen BEFORE there is a response: a
/// bad handle, unreadable JSON, a panic.
///
/// # Safety
///
/// `handle` must be an open engine, `request` a valid NUL-terminated UTF-8 string, and
/// `out` a writable pointer to one string.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn gct_look_ahead(
    handle: *mut Engine,
    request: *const c_char,
    out: *mut *mut c_char,
) -> c_int {
    guarded(|| {
        let (Some(engine), false) = (unsafe { engine(handle) }, out.is_null()) else {
            return GCT_BAD_HANDLE;
        };
        let Some(text) = (unsafe { borrowed(request) }) else {
            return GCT_BAD_ARGUMENT;
        };

        match engine.0.look_ahead(text) {
            Ok(response) => write_json(&response, out),
            Err(status) => status as c_int,
        }
    })
}

/// Hands out a plain string the caller owns.
///
/// For the answers that are not JSON. Same ownership rule as everything else here: the
/// caller frees it with [`gct_string_free`] and never with its own allocator.
fn write_text(value: &str, out: *mut *mut c_char) -> c_int {
    let Ok(owned) = CString::new(value) else {
        return GCT_SERIALISE_FAILED;
    };

    unsafe { *out = owned.into_raw() };
    GCT_OK
}

/// Serialises `value` into a string the caller owns.
///
/// A serialisation that fails is reported rather than unwrapped: these are plain data
/// types and it should not happen, but "should not happen" is not a reason to take the
/// game down.
fn write_json<T: serde::Serialize>(value: &T, out: *mut *mut c_char) -> c_int {
    let Ok(text) = serde_json::to_string(value) else {
        return GCT_SERIALISE_FAILED;
    };
    // A NUL inside would truncate the string on the other side. Nothing here can produce
    // one - JSON escapes it - but the conversion is the place that would find out.
    let Ok(owned) = CString::new(text) else {
        return GCT_SERIALISE_FAILED;
    };

    unsafe { *out = owned.into_raw() };
    GCT_OK
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
        let code = unsafe { gct_engine_open(path.as_ptr(), ptr::null(), &mut handle) };

        assert_eq!(code, GCT_INDEX_UNREADABLE);
        assert!(handle.is_null(), "a failed open must not hand out a handle");
    }

    #[test]
    fn a_null_path_is_a_bad_argument_rather_than_a_crash() {
        let mut handle: *mut Engine = ptr::null_mut();
        assert_eq!(
            unsafe { gct_engine_open(ptr::null(), ptr::null(), &mut handle) },
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

    #[test]
    fn a_null_handle_is_refused_by_the_json_calls_too() {
        let mut out: *mut c_char = ptr::null_mut();
        assert_eq!(
            unsafe { gct_questions(ptr::null_mut(), 631, &mut out) },
            GCT_BAD_HANDLE,
        );

        let request = CString::new("{}").unwrap();
        assert_eq!(
            unsafe { gct_look_ahead(ptr::null_mut(), request.as_ptr(), &mut out) },
            GCT_BAD_HANDLE,
        );
        assert!(out.is_null(), "a refused call must not hand out a string");
    }

    /// A request that is not JSON is refused, rather than panicking across the boundary.
    ///
    /// The caller here is a modded game, and the whole point of the status codes is that
    /// its failures are legible instead of fatal.
    #[test]
    fn a_request_that_is_not_json_is_a_bad_argument() {
        // No index needed: the handle is checked first, so this uses a real engine only
        // where one is required. Here the argument is what is wrong.
        let engine = Box::into_raw(Box::new(Engine(Service::empty())));
        let request = CString::new("not json at all").unwrap();
        let mut out: *mut c_char = ptr::null_mut();

        let code = unsafe { gct_look_ahead(engine, request.as_ptr(), &mut out) };
        assert_eq!(code, GCT_BAD_ARGUMENT);
        assert!(out.is_null());

        unsafe { gct_engine_close(engine) };
    }

    /// Asking about a conversation the index does not hold says so.
    #[test]
    fn questions_about_an_absent_conversation_are_refused() {
        let engine = Box::into_raw(Box::new(Engine(Service::empty())));
        let mut out: *mut c_char = ptr::null_mut();

        let code = unsafe { gct_questions(engine, 631, &mut out) };
        assert_eq!(code, GCT_NO_SUCH_CONVERSATION);

        unsafe { gct_engine_close(engine) };
    }

    /// A string handed out is readable and freeable, which is the contract the caller
    /// relies on for every JSON answer.
    #[test]
    fn a_json_answer_round_trips_and_frees() {
        let engine = Box::into_raw(Box::new(Engine(Service::empty())));
        // An empty group answers nothing, but the request is well-formed, so the response
        // is a real one - which is what this is checking the handling of.
        let request = CString::new(
            r#"{"conversation":1,"starts":[],"world":{"money":0,"day_minutes":0,
               "day_counter":1,"clock_locked":false}}"#,
        )
        .unwrap();
        let mut out: *mut c_char = ptr::null_mut();

        let code = unsafe { gct_look_ahead(engine, request.as_ptr(), &mut out) };
        assert_eq!(code, GCT_OK);
        assert!(!out.is_null());

        let text = unsafe { CStr::from_ptr(out) }.to_str().unwrap().to_string();
        assert!(text.contains("\"answers\""), "got {text}");

        unsafe { gct_string_free(out) };
        unsafe { gct_engine_close(engine) };
    }
}
