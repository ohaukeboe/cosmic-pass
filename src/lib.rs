//! COSMIC Pass: a quick-access popup for Proton Pass on the COSMIC desktop.

pub mod app;
pub mod cache;
pub mod clipboard;
pub mod config;
pub mod core;
pub mod model;
pub mod pass;
#[cfg(any(test, feature = "testing"))]
pub mod testing;
