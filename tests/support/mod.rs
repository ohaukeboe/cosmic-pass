//! Shared helpers for the tests that drive the real `pass-cli`.
//!
//! A `tests/` *subdirectory* is compiled into each test binary that declares `mod support;`
//! rather than becoming a test binary of its own, which a top-level `tests/support.rs` would.
//!
//! Each binary uses a different subset of this module -- the contract suite never builds a
//! `LiveScenario`, the live suite never asserts on isolation -- and every binary compiles the
//! whole module, so unused items here are expected rather than dead.
#![allow(dead_code)]

pub mod real_cli;
