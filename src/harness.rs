//! Headless test harness.
//!
//! `dev/PLAN.md` §4.2 / §7.2: a single entry point an agent (or a test) uses to drive ciq
//! **without a terminal**. `EngineHarness` (load a fixture once, fire SQL, assert on
//! `QueryOutcome`) and the App-level `AppHarness` (render `App` to `ratatui::TestBackend`,
//! returning the serialized buffer). `AppHarness` grows a key-event feed + `current_time_ms`
//! debounce seam + worker pumping in Phase 2.
//!
//! This module is the testing seam made concrete: everything here is pure-plus-fixtures, no
//! PTY, no spawned process, no network.

pub mod app_harness;
pub mod engine_harness;

pub use app_harness::AppHarness;
pub use engine_harness::EngineHarness;

#[cfg(test)]
#[path = "harness/engine_harness_tests.rs"]
mod engine_harness_tests;

#[cfg(test)]
#[path = "harness/app_harness_tests.rs"]
mod app_harness_tests;
