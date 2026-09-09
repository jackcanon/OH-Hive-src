//! Moved to `ohhive-core::setup` (ADR-018 decision 2, 2026-09-08): this Tauri crate and the
//! native Swift shell's `ohhive-ffi` crate now share one implementation of first-run assessment
//! and Ollama install/pull instead of each having their own copy. `lib.rs` does
//! `use ohhive_core::setup;` so nothing else in this crate needed to change.
//!
//! This file is intentionally empty and unreferenced (not declared with `mod setup;` anymore) --
//! kept only so its git history stays attached to this path rather than looking like a deletion.
