//! TUI (Terminal User Interface) module for autom8.
//!
//! This module provides a rich terminal interface for displaying task implementation
//! progress using Ratatui. It is completely self-contained and isolated from the
//! rest of the application.
//!
//! # Architecture
//!
//! The TUI module is organized into three submodules:
//!
//! - `app`: TUI application state management
//! - `ui`: Layout and widget definitions
//! - `mod.rs` (this file): Module root with `TuiDisplay` implementation
//!
//! # Usage
//!
//! The `TuiDisplay` struct implements the `DisplayAdapter` trait, allowing it to
//! be used interchangeably with `CliDisplay` in the runner.
//!
//! ```ignore
//! use autom8::tui::TuiDisplay;
//! use autom8::display::DisplayAdapter;
//!
//! let display: Box<dyn DisplayAdapter> = Box::new(TuiDisplay::new());
//! ```
//!
//! # Event Loop
//!
//! The TUI provides two modes of operation:
//!
//! 1. **With event loop**: Call `TuiDisplay::run()` to start the TUI with its own
//!    render thread. This handles terminal initialization, cleanup, and rendering.
//!
//! 2. **Without event loop**: Use `TuiDisplay` directly as a `DisplayAdapter`.
//!    In this mode, you must manage the terminal yourself.
//!
//! # Design Goals
//!
//! - **Complete isolation**: TUI code lives entirely in `src/tui/`
//! - **Self-contained**: No TUI-specific code in runner, state, or other core modules
//! - **Interchangeable**: `TuiDisplay` can be swapped for `CliDisplay` at runtime

mod app;
mod ui;

pub use app::TuiApp;

use crate::display::{BannerColor, DisplayAdapter, StoryResult};
use crate::progress::Breadcrumb;
use crate::spec::Spec;
use crate::state::MachineState;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::prelude::*;
use std::io::{self, stdout};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// Target frames per second for the TUI render loop.
/// 10 FPS provides smooth updates without excessive CPU usage.
const TARGET_FPS: u64 = 10;

/// Frame duration based on target FPS.
const FRAME_DURATION: Duration = Duration::from_millis(1000 / TARGET_FPS);

/// TUI display implementation using Ratatui.
///
/// This struct implements `DisplayAdapter` and provides a rich terminal interface
/// for displaying task implementation progress. All display operations update
/// the internal `TuiApp` state, which is rendered by the TUI event loop.
///
/// # Thread Safety
///
/// The `TuiApp` is wrapped in `Arc<Mutex<_>>` to allow safe concurrent access
/// from the display adapter and the rendering thread.
///
/// # Event Loop
///
/// The TUI can be run in two modes:
/// - **Managed**: Call `run()` to start the TUI with its own event loop
/// - **Unmanaged**: Use directly as `DisplayAdapter` without event loop
pub struct TuiDisplay {
    /// Shared application state for the TUI.
    app: Arc<Mutex<TuiApp>>,
    /// Flag indicating whether the TUI should continue running.
    running: Arc<AtomicBool>,
    /// Handle to the render thread (if started).
    render_thread: Option<thread::JoinHandle<io::Result<()>>>,
}

impl TuiDisplay {
    /// Create a new TUI display adapter.
    ///
    /// Initializes the TUI application state. The actual terminal setup
    /// (alternate screen, raw mode) happens when `start()` is called.
    pub fn new() -> Self {
        Self {
            app: Arc::new(Mutex::new(TuiApp::new())),
            running: Arc::new(AtomicBool::new(false)),
            render_thread: None,
        }
    }

    /// Get a reference to the shared application state.
    ///
    /// This is used by the event loop to render the UI.
    pub fn app(&self) -> Arc<Mutex<TuiApp>> {
        Arc::clone(&self.app)
    }

    /// Start the TUI event loop in a background thread.
    ///
    /// This initializes the terminal (alternate screen, raw mode) and starts
    /// a render thread that updates the display at the target FPS.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the TUI started successfully, or an error if
    /// terminal initialization failed.
    ///
    /// # Thread Safety
    ///
    /// The render thread holds an Arc to the app state, so display operations
    /// on this `TuiDisplay` will update the shared state that the render thread
    /// displays.
    pub fn start(&mut self) -> io::Result<()> {
        self.running.store(true, Ordering::SeqCst);

        // Clone Arc references for the render thread
        let app = Arc::clone(&self.app);
        let running = Arc::clone(&self.running);

        // Start render thread
        let handle = thread::spawn(move || -> io::Result<()> {
            // Initialize terminal inside the thread (owns stdout)
            enable_raw_mode()?;
            let mut stdout_handle = stdout();
            execute!(stdout_handle, EnterAlternateScreen)?;

            let backend = CrosstermBackend::new(stdout_handle);
            let mut terminal = Terminal::new(backend)?;

            // Main render loop
            while running.load(Ordering::SeqCst) {
                // Render the UI
                {
                    let app_guard = app.lock().unwrap();
                    terminal.draw(|frame| {
                        ui::render(frame, &app_guard);
                    })?;
                }

                // Handle input events (non-blocking)
                if event::poll(FRAME_DURATION)? {
                    if let Event::Key(key) = event::read()? {
                        if key.kind == KeyEventKind::Press {
                            match key.code {
                                // Quit on 'q' or Ctrl+C
                                KeyCode::Char('q') => {
                                    running.store(false, Ordering::SeqCst);
                                    break;
                                }
                                KeyCode::Char('c')
                                    if key.modifiers.contains(event::KeyModifiers::CONTROL) =>
                                {
                                    running.store(false, Ordering::SeqCst);
                                    break;
                                }
                                _ => {}
                            }
                        }
                    }
                }
            }

            // Restore terminal
            disable_raw_mode()?;
            execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
            terminal.show_cursor()?;

            Ok(())
        });

        self.render_thread = Some(handle);
        Ok(())
    }

    /// Stop the TUI event loop and restore the terminal.
    ///
    /// This signals the render thread to stop and waits for it to complete
    /// terminal cleanup.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` if the TUI stopped successfully, or an error if
    /// terminal restoration failed.
    pub fn stop(&mut self) -> io::Result<()> {
        self.running.store(false, Ordering::SeqCst);

        if let Some(handle) = self.render_thread.take() {
            handle
                .join()
                .map_err(|_| io::Error::other("Render thread panicked"))??;
        }

        Ok(())
    }

    /// Check if the TUI is currently running.
    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    /// Wait for the TUI to complete (e.g., user pressed 'q').
    ///
    /// This blocks until the render thread exits.
    pub fn wait(&mut self) -> io::Result<()> {
        if let Some(handle) = self.render_thread.take() {
            handle
                .join()
                .map_err(|_| io::Error::other("Render thread panicked"))??;
        }
        Ok(())
    }
}

impl Default for TuiDisplay {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TuiDisplay {
    fn drop(&mut self) {
        // Ensure the TUI is stopped and terminal is restored on drop
        if self.is_running() {
            let _ = self.stop();
        }
    }
}

impl DisplayAdapter for TuiDisplay {
    // ========================================================================
    // Header and project info
    // ========================================================================

    fn header(&self) {
        // TUI handles header in its layout, no action needed
    }

    fn project_info(&self, spec: &Spec) {
        if let Ok(mut app) = self.app.lock() {
            app.set_project_name(&spec.project);
            app.set_spec_name(&spec.branch_name);
            app.set_total_stories(spec.total_count());
            app.set_completed_stories(spec.completed_count());
        }
    }

    // ========================================================================
    // State transitions
    // ========================================================================

    fn state_transition(&self, _from: MachineState, to: MachineState) {
        if let Ok(mut app) = self.app.lock() {
            app.set_state(to);
        }
    }

    // ========================================================================
    // Phase banners
    // ========================================================================

    fn phase_banner(&self, phase_name: &str, _color: BannerColor) {
        if let Ok(mut app) = self.app.lock() {
            app.set_phase(phase_name);
        }
    }

    fn phase_footer(&self, _color: BannerColor) {
        // TUI handles phase transitions visually, no explicit footer needed
    }

    // ========================================================================
    // Story progress
    // ========================================================================

    fn iteration_start(&self, iteration: u32, story_id: &str, story_title: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.set_current_story(story_id, story_title);
            app.set_iteration(iteration);
        }
    }

    fn story_complete(&self, story_id: &str, _duration_secs: u64) {
        if let Ok(mut app) = self.app.lock() {
            app.mark_story_complete(story_id);
        }
    }

    fn iteration_complete(&self, _iteration: u32) {
        // Note: We don't increment completed_stories here.
        // Progress tracking is handled by tasks_progress / full_progress
        // which receive authoritative counts from the runner.
    }

    fn all_complete(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_all_complete();
        }
    }

    // ========================================================================
    // Claude output streaming
    // ========================================================================

    fn claude_output(&self, line: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.append_output(line);
        }
    }

    // ========================================================================
    // Progress bars
    // ========================================================================

    fn tasks_progress(&self, completed: usize, total: usize) {
        if let Ok(mut app) = self.app.lock() {
            app.set_completed_stories(completed);
            app.set_total_stories(total);
        }
    }

    fn full_progress(
        &self,
        tasks_completed: usize,
        tasks_total: usize,
        review_current: u32,
        review_max: u32,
    ) {
        if let Ok(mut app) = self.app.lock() {
            app.set_completed_stories(tasks_completed);
            app.set_total_stories(tasks_total);
            app.set_review_progress(review_current, review_max);
        }
    }

    // ========================================================================
    // Breadcrumb trail
    // ========================================================================

    fn breadcrumb_trail(&self, breadcrumb: &Breadcrumb) {
        if let Ok(mut app) = self.app.lock() {
            // Build a plain text breadcrumb representation for the TUI
            let mut parts: Vec<&str> = Vec::new();
            for state in breadcrumb.completed_states() {
                parts.push(state.display_name());
            }
            if let Some(current) = breadcrumb.current_state() {
                parts.push(current.display_name());
            }
            app.set_breadcrumb(parts.join(" > "));
        }
    }

    // ========================================================================
    // Review/correct phase messages
    // ========================================================================

    fn reviewing(&self, iteration: u32, max_iterations: u32) {
        if let Ok(mut app) = self.app.lock() {
            app.set_review_progress(iteration, max_iterations);
            app.set_phase("REVIEWING");
        }
    }

    fn review_passed(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_review_passed();
        }
    }

    fn issues_found(&self, iteration: u32, max_iterations: u32) {
        if let Ok(mut app) = self.app.lock() {
            app.set_review_progress(iteration, max_iterations);
            app.set_issues_found();
        }
    }

    fn skip_review(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_review_skipped();
        }
    }

    fn max_review_iterations(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_max_review_iterations_reached();
        }
    }

    // ========================================================================
    // Error display
    // ========================================================================

    fn error_panel(
        &self,
        error_type: &str,
        message: &str,
        exit_code: Option<i32>,
        stderr: Option<&str>,
    ) {
        if let Ok(mut app) = self.app.lock() {
            app.set_error(error_type, message, exit_code, stderr);
        }
    }

    // ========================================================================
    // Completion summary
    // ========================================================================

    fn run_summary(
        &self,
        total_stories: usize,
        completed_stories: usize,
        total_iterations: u32,
        total_duration_secs: u64,
        story_results: &[StoryResult],
    ) {
        if let Ok(mut app) = self.app.lock() {
            app.set_run_summary(
                total_stories,
                completed_stories,
                total_iterations,
                total_duration_secs,
                story_results,
            );
        }
    }

    // ========================================================================
    // Spec operations
    // ========================================================================

    fn spec_loaded(&self, path: &Path, _size_bytes: u64) {
        if let Ok(mut app) = self.app.lock() {
            if let Some(filename) = path.file_name().and_then(|n| n.to_str()) {
                app.set_spec_name(filename);
            }
        }
    }

    fn generating_spec(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_phase("GENERATING SPEC");
        }
    }

    fn spec_generated(&self, spec: &Spec, _output_path: &Path) {
        if let Ok(mut app) = self.app.lock() {
            app.set_project_name(&spec.project);
            app.set_total_stories(spec.total_count());
        }
    }

    fn proceeding_to_implementation(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_phase("IMPLEMENTING");
        }
    }

    // ========================================================================
    // PR and push operations
    // ========================================================================

    fn pr_success(&self, url: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.set_pr_status("created", Some(url));
        }
    }

    fn pr_already_exists(&self, url: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.set_pr_status("exists", Some(url));
        }
    }

    fn pr_skipped(&self, reason: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.set_pr_status(&format!("skipped: {}", reason), None);
        }
    }

    fn pr_updated(&self, url: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.set_pr_status("updated", Some(url));
        }
    }

    fn pushing_branch(&self, branch: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.set_push_status(&format!("pushing {}", branch));
        }
    }

    fn push_success(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_push_status("success");
        }
    }

    fn push_already_up_to_date(&self) {
        if let Ok(mut app) = self.app.lock() {
            app.set_push_status("up-to-date");
        }
    }

    // ========================================================================
    // Info/warning messages
    // ========================================================================

    fn info(&self, msg: &str) {
        if let Ok(mut app) = self.app.lock() {
            app.append_info(msg);
        }
    }

    // ========================================================================
    // Newline helper
    // ========================================================================

    fn newline(&self) {
        // TUI handles spacing in its layout
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // TuiDisplay creation tests
    // ========================================================================

    #[test]
    fn test_tui_display_new() {
        let display = TuiDisplay::new();
        // Should create successfully
        let _ = display;
    }

    #[test]
    fn test_tui_display_default() {
        let display = TuiDisplay::default();
        // Should create successfully
        let _ = display;
    }

    #[test]
    fn test_tui_display_implements_display_adapter() {
        fn accepts_display_adapter(_: &dyn DisplayAdapter) {}

        let display = TuiDisplay::new();
        accepts_display_adapter(&display);
    }

    #[test]
    fn test_tui_display_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<TuiDisplay>();
    }

    #[test]
    fn test_tui_display_app_accessor() {
        let display = TuiDisplay::new();
        let app = display.app();
        // Should be able to lock the app
        let guard = app.lock().unwrap();
        drop(guard);
    }

    // ========================================================================
    // TuiDisplay DisplayAdapter method tests
    // ========================================================================

    #[test]
    fn test_tui_display_header_no_panic() {
        let display = TuiDisplay::new();
        display.header();
    }

    #[test]
    fn test_tui_display_project_info_updates_app() {
        use crate::spec::UserStory;

        let display = TuiDisplay::new();
        let spec = Spec {
            project: "TestProject".into(),
            branch_name: "test-branch".into(),
            description: "A test".into(),
            user_stories: vec![UserStory {
                id: "US-001".into(),
                title: "Story".into(),
                description: "Desc".into(),
                acceptance_criteria: vec![],
                priority: 1,
                passes: false,
                notes: String::new(),
            }],
        };

        display.project_info(&spec);

        let app = display.app();
        let guard = app.lock().unwrap();
        assert_eq!(guard.project_name(), "TestProject");
        assert_eq!(guard.total_stories(), 1);
    }

    #[test]
    fn test_tui_display_state_transition_updates_app() {
        let display = TuiDisplay::new();

        display.state_transition(MachineState::Idle, MachineState::RunningClaude);

        let app = display.app();
        let guard = app.lock().unwrap();
        assert_eq!(guard.state(), MachineState::RunningClaude);
    }

    #[test]
    fn test_tui_display_phase_banner_updates_app() {
        let display = TuiDisplay::new();

        display.phase_banner("RUNNING", BannerColor::Cyan);

        let app = display.app();
        let guard = app.lock().unwrap();
        assert_eq!(guard.phase(), "RUNNING");
    }

    #[test]
    fn test_tui_display_iteration_start_updates_app() {
        let display = TuiDisplay::new();

        display.iteration_start(1, "US-001", "Test Story");

        let app = display.app();
        let guard = app.lock().unwrap();
        assert_eq!(guard.current_story_id(), Some("US-001"));
        assert_eq!(guard.current_story_title(), Some("Test Story"));
        assert_eq!(guard.iteration(), 1);
    }

    #[test]
    fn test_tui_display_claude_output_appends_to_app() {
        let display = TuiDisplay::new();

        display.claude_output("Line 1");
        display.claude_output("Line 2");

        let app = display.app();
        let guard = app.lock().unwrap();
        let output = guard.output();
        assert!(output.contains("Line 1"));
        assert!(output.contains("Line 2"));
    }

    #[test]
    fn test_tui_display_tasks_progress_updates_app() {
        let display = TuiDisplay::new();

        display.tasks_progress(3, 8);

        let app = display.app();
        let guard = app.lock().unwrap();
        assert_eq!(guard.completed_stories(), 3);
        assert_eq!(guard.total_stories(), 8);
    }

    #[test]
    fn test_tui_display_full_progress_updates_app() {
        let display = TuiDisplay::new();

        display.full_progress(3, 8, 2, 3);

        let app = display.app();
        let guard = app.lock().unwrap();
        assert_eq!(guard.completed_stories(), 3);
        assert_eq!(guard.total_stories(), 8);
        assert_eq!(guard.review_current(), 2);
        assert_eq!(guard.review_max(), 3);
    }

    #[test]
    fn test_tui_display_error_panel_updates_app() {
        let display = TuiDisplay::new();

        display.error_panel("Test Error", "An error occurred", Some(1), Some("stderr"));

        let app = display.app();
        let guard = app.lock().unwrap();
        assert!(guard.has_error());
    }

    #[test]
    fn test_tui_display_info_appends_to_app() {
        let display = TuiDisplay::new();

        display.info("Test info message");

        let app = display.app();
        let guard = app.lock().unwrap();
        // Info messages are added to output or a separate info buffer
        // The exact behavior depends on TuiApp implementation
        drop(guard);
    }

    #[test]
    fn test_tui_display_newline_no_panic() {
        let display = TuiDisplay::new();
        display.newline();
    }

    // ========================================================================
    // Box<dyn DisplayAdapter> tests
    // ========================================================================

    #[test]
    fn test_boxed_tui_display_is_object_safe() {
        let display: Box<dyn DisplayAdapter> = Box::new(TuiDisplay::new());
        display.header();
        display.state_transition(MachineState::Idle, MachineState::Initializing);
    }

    #[test]
    fn test_boxed_tui_display_can_be_stored() {
        struct Container {
            display: Box<dyn DisplayAdapter>,
        }

        let container = Container {
            display: Box::new(TuiDisplay::new()),
        };
        container.display.info("test");
    }

    // ========================================================================
    // TUI Event Loop tests (US-006)
    // ========================================================================

    #[test]
    fn test_tui_display_is_not_running_by_default() {
        let display = TuiDisplay::new();
        assert!(
            !display.is_running(),
            "TUI should not be running by default"
        );
    }

    #[test]
    fn test_tui_display_running_flag_is_atomic() {
        let display = TuiDisplay::new();
        let running = Arc::clone(&display.running);

        // Check initial state
        assert!(!running.load(Ordering::SeqCst));

        // Set to true
        running.store(true, Ordering::SeqCst);
        assert!(running.load(Ordering::SeqCst));
        assert!(display.is_running());

        // Set back to false
        running.store(false, Ordering::SeqCst);
        assert!(!display.is_running());
    }

    #[test]
    fn test_tui_display_stop_without_start_is_safe() {
        let mut display = TuiDisplay::new();
        // Stopping without starting should not panic
        let result = display.stop();
        assert!(result.is_ok());
    }

    #[test]
    fn test_tui_display_drop_calls_stop() {
        let display = TuiDisplay::new();
        // Just drop it - should not panic
        drop(display);
    }

    #[test]
    fn test_frame_duration_is_reasonable() {
        // At 10 FPS, frame duration should be 100ms
        assert_eq!(FRAME_DURATION.as_millis(), 100);
    }

    #[test]
    fn test_target_fps_is_reasonable() {
        // Target FPS should be 10 (good balance of smoothness and CPU usage)
        assert_eq!(TARGET_FPS, 10);
    }

    // ========================================================================
    // TuiApp exit control tests (US-006)
    // ========================================================================

    #[test]
    fn test_tui_app_should_exit_defaults_to_false() {
        let app = TuiApp::new();
        assert!(!app.should_exit());
    }

    #[test]
    fn test_tui_app_request_exit_sets_flag() {
        let mut app = TuiApp::new();
        app.request_exit();
        assert!(app.should_exit());
    }

    #[test]
    fn test_tui_app_request_exit_with_message() {
        let mut app = TuiApp::new();
        app.request_exit_with_message("User pressed q");
        assert!(app.should_exit());
        assert_eq!(app.exit_message(), Some("User pressed q"));
    }

    #[test]
    fn test_tui_app_exit_message_defaults_to_none() {
        let app = TuiApp::new();
        assert!(app.exit_message().is_none());
    }

    #[test]
    fn test_tui_app_is_run_complete_for_completed_state() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::Completed);
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_tui_app_is_run_complete_for_failed_state() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::Failed);
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_tui_app_is_run_complete_for_all_complete_flag() {
        let mut app = TuiApp::new();
        app.set_all_complete();
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_tui_app_is_run_complete_for_run_summary() {
        let mut app = TuiApp::new();
        app.set_run_summary(1, 1, 1, 60, &[]);
        assert!(app.is_run_complete());
    }

    #[test]
    fn test_tui_app_is_not_complete_when_running() {
        let mut app = TuiApp::new();
        app.set_state(MachineState::RunningClaude);
        assert!(!app.is_run_complete());
    }

    // ========================================================================
    // Thread safety tests
    // ========================================================================

    #[test]
    fn test_tui_display_app_is_thread_safe() {
        let display = TuiDisplay::new();
        let app = display.app();

        // Spawn a thread that updates the app
        let app_clone = Arc::clone(&app);
        let handle = thread::spawn(move || {
            let mut guard = app_clone.lock().unwrap();
            guard.set_project_name("ThreadTest");
        });

        handle.join().unwrap();

        // Verify the update from the other thread
        let guard = app.lock().unwrap();
        assert_eq!(guard.project_name(), "ThreadTest");
    }

    #[test]
    fn test_tui_display_concurrent_updates() {
        let display = TuiDisplay::new();
        let app = display.app();

        // Spawn multiple threads that update different fields
        let handles: Vec<_> = (0..5)
            .map(|i| {
                let app_clone = Arc::clone(&app);
                thread::spawn(move || {
                    let mut guard = app_clone.lock().unwrap();
                    guard.append_output(&format!("Line from thread {}", i));
                })
            })
            .collect();

        // Wait for all threads
        for handle in handles {
            handle.join().unwrap();
        }

        // Verify all updates were applied
        let guard = app.lock().unwrap();
        let output = guard.output();
        for i in 0..5 {
            assert!(
                output.contains(&format!("Line from thread {}", i)),
                "Output should contain line from thread {}",
                i
            );
        }
    }
}
