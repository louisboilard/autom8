//! Process monitoring and management utilities.
//!
//! This module provides infrastructure for monitoring CPU and memory usage
//! of spawned subprocess (e.g., Claude CLI processes).

mod monitor;

pub use monitor::{ProcessMetrics, ProcessMonitor};
