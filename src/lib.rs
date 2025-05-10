// src/lib.rs

//! # Hirai
//!
//! Hirai is a cross-platform file change broadcaster and listener using UDP multicast and optional WebSocket/web UI.
//!
//! ## Modules
//!
//! - [`config`]: Configuration loading and merging from CLI, file, and environment.
//! - [`event`]: File event struct and serialization.
//! - [`network`]: UDP multicast broadcaster and listener logic.
//! - [`watcher`]: File system watcher for change detection.
//! - [`web`]: Web server and WebSocket event delivery.

pub mod config;
pub mod event;
pub mod network;
pub mod watcher;
pub mod web;
