// SPDX-License-Identifier: MIT
//! The types that cross the pipe, generated from `proto/engine.proto`.
//!
//! ## Nothing here is written by hand
//!
//! The module's whole body is the generated one, pulled in by the include below. The
//! schema is the source, this file is where it lands, and an edit made here would be gone
//! on the next build - which is the point: the shape used to exist twice, as Rust types
//! with serde derives and as C# classes writing the same members by hand, and the two
//! agreed only because someone remembered.
//!
//! ## The name
//!
//! `gct.engine.v1.rs` is prost's file for the schema's `package gct.engine.v1`, so the
//! package name and this path move together. A `v1` that has to become a `v2` is a rename
//! in one file and a rebuild, rather than a search.
//!
//! ## What is NOT generated
//!
//! The conversions between these types and the engine's own - `NodeRef` to the graph's
//! ids, a `WireValue` to a `GuardValue`, a run-encoded `NodeSet` to a set. Those live in
//! [`crate::bridge`], because they are about what the engine means by something rather
//! than about what crosses.

include!(concat!(env!("OUT_DIR"), "/gct.engine.v1.rs"));
