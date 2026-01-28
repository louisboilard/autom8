//! Monitor TUI - A standalone terminal UI for monitoring autom8 activity.
//!
//! This module provides a real-time dashboard view of autom8 activity across all projects.
//! It polls state files at regular intervals and displays:
//! - Active runs across all projects
//! - Project list with status
//! - Run history
//!
//! The interface is read-only with simple keyboard navigation:
//! - Tab: Switch between views
//! - Arrow keys: Navigate within a view
//! - Enter: Select/expand items
//! - Q: Quit

pub mod app;
pub mod views;

pub use app::{run_monitor, MonitorApp, MonitorResult};
