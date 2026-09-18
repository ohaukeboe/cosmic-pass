//! COSMIC Pass: a quick-access popup for Proton Pass on the COSMIC desktop.

pub mod app;
pub mod cache;
pub mod cli;
pub mod clipboard;
pub mod config;
pub mod core;
pub mod model;
pub mod pass;
pub mod runtime;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
