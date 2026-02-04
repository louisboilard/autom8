//! Process resource monitoring using sysinfo.
//!
//! Provides CPU and memory monitoring for tracked processes.

use sysinfo::{Pid, System};

/// Holds resource usage metrics for a process.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessMetrics {
    /// CPU usage as a percentage (0.0 - 100.0+, can exceed 100% on multi-core)
    pub cpu_percent: f32,
    /// Memory usage in bytes
    pub memory_bytes: u64,
    /// Memory usage as a percentage of total system memory (0.0 - 100.0)
    pub memory_percent: f32,
}

impl ProcessMetrics {
    /// Creates a new ProcessMetrics instance.
    pub fn new(cpu_percent: f32, memory_bytes: u64, memory_percent: f32) -> Self {
        Self {
            cpu_percent,
            memory_bytes,
            memory_percent,
        }
    }
}

impl Default for ProcessMetrics {
    fn default() -> Self {
        Self {
            cpu_percent: 0.0,
            memory_bytes: 0,
            memory_percent: 0.0,
        }
    }
}

/// Monitors resource usage for a specific process by PID.
///
/// Wraps `sysinfo::System` and tracks metrics for a single process.
/// The monitor handles the case where the process may exit between checks.
pub struct ProcessMonitor {
    system: System,
    pid: Pid,
    total_memory: u64,
    last_metrics: Option<ProcessMetrics>,
}

impl ProcessMonitor {
    /// Creates a new ProcessMonitor for the given PID.
    ///
    /// The monitor is initialized but metrics are not populated until
    /// `refresh()` is called.
    pub fn new(pid: u32) -> Self {
        let mut system = System::new();
        // Must refresh memory info to get total_memory
        system.refresh_memory();
        let total_memory = system.total_memory();

        Self {
            system,
            pid: Pid::from_u32(pid),
            total_memory,
            last_metrics: None,
        }
    }

    /// Returns the PID being monitored.
    pub fn pid(&self) -> u32 {
        self.pid.as_u32()
    }

    /// Refreshes metrics for the tracked process.
    ///
    /// This method queries the OS for current CPU and memory usage.
    /// If the process no longer exists, the internal metrics are cleared.
    ///
    /// Call this at regular intervals (e.g., 500ms) to get updated metrics.
    pub fn refresh(&mut self) {
        // Refresh only the specific process we're tracking
        self.system.refresh_processes_specifics(
            sysinfo::ProcessesToUpdate::Some(&[self.pid]),
            true, // refresh CPU info
            sysinfo::ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory(),
        );

        // Get the process if it exists
        if let Some(process) = self.system.process(self.pid) {
            let cpu_percent = process.cpu_usage();
            let memory_bytes = process.memory();
            let memory_percent = if self.total_memory > 0 {
                (memory_bytes as f64 / self.total_memory as f64 * 100.0) as f32
            } else {
                0.0
            };

            self.last_metrics = Some(ProcessMetrics {
                cpu_percent,
                memory_bytes,
                memory_percent,
            });
        } else {
            // Process no longer exists
            self.last_metrics = None;
        }
    }

    /// Returns the current metrics for the tracked process.
    ///
    /// Returns `None` if:
    /// - `refresh()` has never been called
    /// - The process has exited since the last refresh
    /// - The process doesn't exist
    pub fn metrics(&self) -> Option<ProcessMetrics> {
        self.last_metrics.clone()
    }

    /// Returns whether the process is still running.
    ///
    /// This is based on the last `refresh()` call. Call `refresh()` first
    /// for an up-to-date check.
    pub fn is_running(&self) -> bool {
        self.last_metrics.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_process_metrics_new() {
        let metrics = ProcessMetrics::new(50.5, 1024 * 1024 * 100, 25.0);

        assert!((metrics.cpu_percent - 50.5).abs() < f32::EPSILON);
        assert_eq!(metrics.memory_bytes, 104857600);
        assert!((metrics.memory_percent - 25.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_process_metrics_default() {
        let metrics = ProcessMetrics::default();

        assert!((metrics.cpu_percent - 0.0).abs() < f32::EPSILON);
        assert_eq!(metrics.memory_bytes, 0);
        assert!((metrics.memory_percent - 0.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_process_metrics_clone() {
        let metrics = ProcessMetrics::new(75.0, 2048, 10.5);
        let cloned = metrics.clone();

        assert_eq!(metrics, cloned);
    }

    #[test]
    fn test_process_metrics_equality() {
        let metrics1 = ProcessMetrics::new(50.0, 1000, 5.0);
        let metrics2 = ProcessMetrics::new(50.0, 1000, 5.0);
        let metrics3 = ProcessMetrics::new(60.0, 1000, 5.0);

        assert_eq!(metrics1, metrics2);
        assert_ne!(metrics1, metrics3);
    }

    #[test]
    fn test_process_monitor_new() {
        let monitor = ProcessMonitor::new(12345);

        assert_eq!(monitor.pid(), 12345);
        assert!(monitor.metrics().is_none());
        assert!(!monitor.is_running());
    }

    #[test]
    fn test_process_monitor_nonexistent_process() {
        // Use an extremely unlikely PID that shouldn't exist
        let mut monitor = ProcessMonitor::new(u32::MAX - 1);

        // Refresh should not panic for nonexistent process
        monitor.refresh();

        // Should return None since process doesn't exist
        assert!(monitor.metrics().is_none());
        assert!(!monitor.is_running());
    }

    #[test]
    fn test_process_monitor_current_process() {
        // Monitor the current test process
        let pid = std::process::id();
        let mut monitor = ProcessMonitor::new(pid);

        // First refresh to initialize
        monitor.refresh();

        // The current process should exist
        assert!(monitor.is_running());
        let metrics = monitor.metrics();
        assert!(metrics.is_some());

        let metrics = metrics.unwrap();
        // Memory should be non-zero for a running process
        assert!(metrics.memory_bytes > 0);
        // Memory percent should be reasonable (> 0, < 100)
        assert!(metrics.memory_percent > 0.0);
        assert!(metrics.memory_percent < 100.0);
    }

    #[test]
    fn test_process_monitor_refresh_updates_metrics() {
        let pid = std::process::id();
        let mut monitor = ProcessMonitor::new(pid);

        // First refresh
        monitor.refresh();
        let metrics1 = monitor.metrics();
        assert!(metrics1.is_some());

        // Second refresh
        monitor.refresh();
        let metrics2 = monitor.metrics();
        assert!(metrics2.is_some());

        // Both should have valid memory values
        assert!(metrics1.unwrap().memory_bytes > 0);
        assert!(metrics2.unwrap().memory_bytes > 0);
    }

    #[test]
    fn test_process_monitor_total_memory() {
        // Total system memory should be reasonable (at least 1GB for any modern system)
        let mut system = System::new();
        system.refresh_memory();
        let total = system.total_memory();
        assert!(total > 1024 * 1024 * 1024); // > 1GB
    }
}
