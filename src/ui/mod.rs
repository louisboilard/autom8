//! UI module for autom8.
//!
//! This module consolidates both GUI and TUI interfaces under a single namespace,
//! with shared data structures and logic in the `shared` submodule.
//!
//! # Submodules
//!
//! - [`gui`] - Native GUI using eframe/egui
//! - [`tui`] - Terminal UI using ratatui
//! - [`shared`] - Shared data types and loading logic

pub mod gui;
pub mod shared;
pub mod tui;
