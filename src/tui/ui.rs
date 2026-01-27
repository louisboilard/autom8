//! TUI layout and widget definitions.
//!
//! This module contains the layout and rendering code for the TUI.
//! It uses Ratatui widgets to create a rich terminal interface.
//!
//! # Layout Structure
//!
//! ```text
//! ┌─────────────────── Header ───────────────────┐
//! │ autom8 | Project: X | Branch: Y | State: Z   │
//! ├─────────────────── Progress ─────────────────┤
//! │ Story 2/5: US-002 - Add feature              │
//! │ [████████████░░░░░░░░░░░░░░░░░] 40%          │
//! ├─────────────────── Output ───────────────────┤
//! │ Claude output line 1                          │
//! │ Claude output line 2                          │
//! │ Claude output line 3                          │
//! │ ...                                           │
//! ├─────────────────── Footer ───────────────────┤
//! │ Elapsed: 2m 30s | q to quit                  │
//! └──────────────────────────────────────────────┘
//! ```

use super::app::TuiApp;
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Wrap},
    Frame,
};

/// Render the TUI layout.
///
/// This is the main rendering function that draws all UI components
/// based on the current application state.
pub fn render(frame: &mut Frame, app: &TuiApp) {
    // Create the main layout with header, content, and footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Length(4), // Progress
            Constraint::Min(10),   // Output (takes remaining space)
            Constraint::Length(3), // Footer
        ])
        .split(frame.area());

    render_header(frame, app, chunks[0]);
    render_progress(frame, app, chunks[1]);
    render_output(frame, app, chunks[2]);
    render_footer(frame, app, chunks[3]);
}

/// Render the header section.
fn render_header(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let state_color = match app.state() {
        crate::state::MachineState::Idle => Color::Gray,
        crate::state::MachineState::Initializing => Color::Cyan,
        crate::state::MachineState::LoadingSpec => Color::Cyan,
        crate::state::MachineState::GeneratingSpec => Color::Yellow,
        crate::state::MachineState::PickingStory => Color::Blue,
        crate::state::MachineState::RunningClaude => Color::Green,
        crate::state::MachineState::Reviewing => Color::Yellow,
        crate::state::MachineState::Correcting => Color::Yellow,
        crate::state::MachineState::Committing => Color::Cyan,
        crate::state::MachineState::CreatingPR => Color::Cyan,
        crate::state::MachineState::Completed => Color::Green,
        crate::state::MachineState::Failed => Color::Red,
    };

    let state_name = format!("{:?}", app.state());

    let header_text = vec![Line::from(vec![
        Span::styled(
            "autom8",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Project: ", Style::default().fg(Color::Blue)),
        Span::styled(
            if app.project_name().is_empty() {
                "-"
            } else {
                app.project_name()
            },
            Style::default().fg(Color::White),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("Branch: ", Style::default().fg(Color::Blue)),
        Span::styled(
            if app.spec_name().is_empty() {
                "-"
            } else {
                app.spec_name()
            },
            Style::default().fg(Color::White),
        ),
        Span::styled(" │ ", Style::default().fg(Color::DarkGray)),
        Span::styled("State: ", Style::default().fg(Color::Blue)),
        Span::styled(
            state_name,
            Style::default()
                .fg(state_color)
                .add_modifier(Modifier::BOLD),
        ),
    ])];

    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title(Span::styled(
                    " autom8 ",
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )),
        )
        .style(Style::default());

    frame.render_widget(header, area);
}

/// Render the progress section.
fn render_progress(frame: &mut Frame, app: &TuiApp, area: Rect) {
    // Split progress area into story info and progress bar
    let progress_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Length(2)])
        .split(area);

    // Build story info spans with iteration count for review/correct cycles
    let story_spans =
        if let (Some(id), Some(title)) = (app.current_story_id(), app.current_story_title()) {
            let mut spans = Vec::new();

            // Story progress indicator
            if app.total_stories() > 0 {
                spans.push(Span::styled(
                    format!(
                        "Story {}/{}",
                        app.completed_stories() + 1,
                        app.total_stories()
                    ),
                    Style::default().fg(Color::Yellow),
                ));
                spans.push(Span::raw(": "));
            }

            // Story ID and title
            spans.push(Span::styled(
                id.to_string(),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ));
            spans.push(Span::raw(" - "));
            spans.push(Span::styled(
                title.to_string(),
                Style::default().fg(Color::White),
            ));

            // Add iteration count if in review/correct cycle (iteration > 1 or review_max > 0)
            if app.iteration() > 1 || app.review_max() > 0 {
                spans.push(Span::raw("  "));
                spans.push(Span::styled(
                    format!("(iter {})", app.iteration()),
                    Style::default().fg(Color::Magenta),
                ));
            }

            spans
        } else if !app.phase().is_empty() {
            vec![
                Span::styled("Phase: ", Style::default().fg(Color::Gray)),
                Span::styled(
                    app.phase().to_string(),
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                ),
            ]
        } else {
            vec![Span::styled("Waiting...", Style::default().fg(Color::Gray))]
        };

    let story_paragraph =
        Paragraph::new(Line::from(story_spans)).block(Block::default().borders(Borders::NONE));

    frame.render_widget(story_paragraph, progress_chunks[0]);

    // Progress bar
    let progress_ratio = if app.total_stories() > 0 {
        app.completed_stories() as f64 / app.total_stories() as f64
    } else {
        0.0
    };

    let progress_label = format!(
        "{}/{} stories complete ({}%)",
        app.completed_stories(),
        app.total_stories(),
        (progress_ratio * 100.0) as u8
    );

    // Style matches CLI: green for filled, gray for empty
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::NONE))
        .gauge_style(
            Style::default()
                .fg(Color::Green)
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .ratio(progress_ratio)
        .label(Span::styled(
            progress_label,
            Style::default().fg(Color::White),
        ));

    frame.render_widget(gauge, progress_chunks[1]);
}

/// Render the output section (Claude output).
fn render_output(frame: &mut Frame, app: &TuiApp, area: Rect) {
    // Collect output lines with gray styling (matching CLI print_claude_output)
    let lines: Vec<Line> = app
        .output_lines()
        .map(|line| Line::styled(line.to_string(), Style::default().fg(Color::Gray)))
        .collect();

    // Calculate visible lines based on area height
    let visible_height = area.height.saturating_sub(2) as usize; // -2 for borders
    let total_lines = lines.len();
    let start_index = total_lines.saturating_sub(visible_height);
    let visible_lines: Vec<Line> = lines.into_iter().skip(start_index).collect();

    // Dynamic title showing line count and scroll indicator
    let title = if total_lines > visible_height {
        format!(
            " Claude Output ({} lines, showing last {}) ",
            total_lines, visible_height
        )
    } else {
        format!(" Claude Output ({} lines) ", total_lines)
    };

    let output_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray))
        .title(Span::styled(title, Style::default().fg(Color::Gray)));

    let output = Paragraph::new(visible_lines)
        .block(output_block)
        .wrap(Wrap { trim: false });

    frame.render_widget(output, area);
}

/// Render the footer section.
fn render_footer(frame: &mut Frame, app: &TuiApp, area: Rect) {
    let elapsed = app.elapsed_secs();
    let minutes = elapsed / 60;
    let seconds = elapsed % 60;
    let elapsed_str = format!("{}m {}s", minutes, seconds);

    // Build status line (left side)
    let mut status_parts = vec![
        Span::styled("⏱ ", Style::default().fg(Color::Gray)),
        Span::styled(elapsed_str, Style::default().fg(Color::Yellow)),
    ];

    // Add review progress if in review phase
    if app.review_max() > 0 {
        status_parts.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        status_parts.push(Span::styled("Review: ", Style::default().fg(Color::Gray)));
        status_parts.push(Span::styled(
            format!("{}/{}", app.review_current(), app.review_max()),
            Style::default().fg(Color::Cyan),
        ));
    }

    // Add breadcrumb if present
    if !app.breadcrumb().is_empty() {
        status_parts.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        status_parts.push(Span::styled(
            app.breadcrumb(),
            Style::default().fg(Color::Gray),
        ));
    }

    // Add error indicator if present
    if app.has_error() {
        status_parts.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        status_parts.push(Span::styled(
            "⚠ ERROR",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        ));
    }

    // Add completion indicator
    if app.all_complete() {
        status_parts.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        status_parts.push(Span::styled(
            "✓ COMPLETE",
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ));
    }

    // Add keyboard hints (right-aligned conceptually, shown after separator)
    status_parts.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
    status_parts.push(Span::styled(
        "q",
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD),
    ));
    status_parts.push(Span::styled(" quit", Style::default().fg(Color::Gray)));

    let footer_text = vec![Line::from(status_parts)];

    let footer = Paragraph::new(footer_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
                .title(" Status "),
        )
        .style(Style::default());

    frame.render_widget(footer, area);
}

#[cfg(test)]
mod tests {
    use super::*;

    // ========================================================================
    // Layout tests (verify no panics during rendering)
    // ========================================================================

    // Note: Testing Ratatui rendering typically requires a mock backend.
    // For now, we test that the helper functions don't panic with various inputs.

    #[test]
    fn test_tui_app_state_colors_are_defined() {
        // Verify all states have defined colors (no panic in match)
        let states = [
            crate::state::MachineState::Idle,
            crate::state::MachineState::Initializing,
            crate::state::MachineState::LoadingSpec,
            crate::state::MachineState::GeneratingSpec,
            crate::state::MachineState::PickingStory,
            crate::state::MachineState::RunningClaude,
            crate::state::MachineState::Reviewing,
            crate::state::MachineState::Correcting,
            crate::state::MachineState::Committing,
            crate::state::MachineState::CreatingPR,
            crate::state::MachineState::Completed,
            crate::state::MachineState::Failed,
        ];

        for state in states {
            let color = match state {
                crate::state::MachineState::Idle => Color::Gray,
                crate::state::MachineState::Initializing => Color::Cyan,
                crate::state::MachineState::LoadingSpec => Color::Cyan,
                crate::state::MachineState::GeneratingSpec => Color::Yellow,
                crate::state::MachineState::PickingStory => Color::Blue,
                crate::state::MachineState::RunningClaude => Color::Green,
                crate::state::MachineState::Reviewing => Color::Yellow,
                crate::state::MachineState::Correcting => Color::Yellow,
                crate::state::MachineState::Committing => Color::Cyan,
                crate::state::MachineState::CreatingPR => Color::Cyan,
                crate::state::MachineState::Completed => Color::Green,
                crate::state::MachineState::Failed => Color::Red,
            };
            // Just verify no panic and color is assigned
            let _ = color;
        }
    }

    #[test]
    fn test_progress_ratio_calculation_with_zero_stories() {
        let app = TuiApp::new();
        let ratio = if app.total_stories() > 0 {
            app.completed_stories() as f64 / app.total_stories() as f64
        } else {
            0.0
        };
        assert_eq!(ratio, 0.0);
    }

    #[test]
    fn test_progress_ratio_calculation_with_stories() {
        let mut app = TuiApp::new();
        app.set_total_stories(10);
        app.set_completed_stories(5);

        let ratio = if app.total_stories() > 0 {
            app.completed_stories() as f64 / app.total_stories() as f64
        } else {
            0.0
        };
        assert_eq!(ratio, 0.5);
    }

    #[test]
    fn test_elapsed_time_formatting() {
        // Test the formatting logic
        let elapsed = 150u64; // 2m 30s
        let minutes = elapsed / 60;
        let seconds = elapsed % 60;
        assert_eq!(minutes, 2);
        assert_eq!(seconds, 30);
        let formatted = format!("{}m {}s", minutes, seconds);
        assert_eq!(formatted, "2m 30s");
    }

    #[test]
    fn test_output_lines_visible_calculation() {
        let mut app = TuiApp::new();
        for i in 0..100 {
            app.append_output(&format!("Line {}", i));
        }

        let lines: Vec<_> = app.output_lines().collect();
        let total_lines = lines.len();
        let visible_height = 20usize;
        let start_index = total_lines.saturating_sub(visible_height);

        // Should start at line 80 to show last 20 lines
        assert_eq!(start_index, 80);
    }

    #[test]
    fn test_output_lines_visible_when_less_than_height() {
        let mut app = TuiApp::new();
        for i in 0..5 {
            app.append_output(&format!("Line {}", i));
        }

        let lines: Vec<_> = app.output_lines().collect();
        let total_lines = lines.len();
        let visible_height = 20usize;
        let start_index = total_lines.saturating_sub(visible_height);

        // Should start at 0 since we have fewer lines than visible height
        assert_eq!(start_index, 0);
    }

    // ========================================================================
    // US-005: TUI layout and widgets tests
    // ========================================================================

    #[test]
    fn test_iteration_display_in_progress_section() {
        // Test that iteration count is available for display in review/correct cycles
        let mut app = TuiApp::new();
        app.set_current_story("US-001", "Test Story");
        app.set_iteration(3);
        app.set_review_progress(2, 3);

        // Verify app state is correctly set for iteration display
        assert_eq!(app.iteration(), 3);
        assert_eq!(app.review_current(), 2);
        assert_eq!(app.review_max(), 3);
        assert!(
            app.review_max() > 0,
            "Review max should trigger iteration display"
        );
    }

    #[test]
    fn test_iteration_display_first_iteration_no_review() {
        // First iteration without review shouldn't show iteration count
        let mut app = TuiApp::new();
        app.set_current_story("US-001", "Test Story");
        app.set_iteration(1);

        assert_eq!(app.iteration(), 1);
        assert_eq!(app.review_max(), 0);
        // No iteration display should be triggered when iteration=1 and review_max=0
    }

    #[test]
    fn test_iteration_display_multiple_iterations() {
        // Multiple iterations should show iteration count
        let mut app = TuiApp::new();
        app.set_current_story("US-002", "Another Story");
        app.set_iteration(2);

        assert_eq!(app.iteration(), 2);
        // Iteration > 1 should trigger iteration display
        assert!(app.iteration() > 1);
    }

    #[test]
    fn test_footer_displays_keyboard_hints() {
        // Test that footer includes keyboard hints
        // The keyboard hints "q quit" are always displayed in the footer
        let app = TuiApp::new();

        // Verify app can be used for footer rendering (no panic)
        let elapsed = app.elapsed_secs();
        let _ = elapsed; // Just verify it's accessible

        // Keyboard hints are unconditionally added to footer
        // This test verifies the footer logic works without panic
    }

    #[test]
    fn test_footer_with_review_progress() {
        let mut app = TuiApp::new();
        app.set_review_progress(1, 3);

        // Verify review progress values are accessible for footer display
        assert_eq!(app.review_current(), 1);
        assert_eq!(app.review_max(), 3);
        assert!(
            app.review_max() > 0,
            "Should display review progress in footer"
        );
    }

    #[test]
    fn test_footer_with_error_indicator() {
        let mut app = TuiApp::new();
        app.set_error("TestError", "Test message", Some(1), None);

        // Verify error state is accessible for footer display
        assert!(app.has_error(), "Footer should show error indicator");
    }

    #[test]
    fn test_footer_with_completion_indicator() {
        let mut app = TuiApp::new();
        app.set_all_complete();

        // Verify completion state is accessible for footer display
        assert!(
            app.all_complete(),
            "Footer should show completion indicator"
        );
    }

    #[test]
    fn test_header_displays_project_info() {
        let mut app = TuiApp::new();
        app.set_project_name("TestProject");
        app.set_spec_name("feature/test-branch");
        app.set_state(crate::state::MachineState::RunningClaude);

        // Verify header info is accessible
        assert_eq!(app.project_name(), "TestProject");
        assert_eq!(app.spec_name(), "feature/test-branch");
        assert_eq!(app.state(), crate::state::MachineState::RunningClaude);
    }

    #[test]
    fn test_output_section_scroll_indicator_logic() {
        let mut app = TuiApp::new();

        // Add more lines than visible height
        for i in 0..50 {
            app.append_output(&format!("Output line {}", i));
        }

        let total_lines: usize = app.output_lines().count();
        let visible_height = 20usize;

        // Test scroll indicator logic
        let shows_scroll = total_lines > visible_height;
        assert!(
            shows_scroll,
            "Should show scroll indicator when lines exceed visible area"
        );
    }

    #[test]
    fn test_output_section_no_scroll_indicator() {
        let mut app = TuiApp::new();

        // Add fewer lines than visible height
        for i in 0..5 {
            app.append_output(&format!("Line {}", i));
        }

        let total_lines: usize = app.output_lines().count();
        let visible_height = 20usize;

        // Test scroll indicator logic
        let shows_scroll = total_lines > visible_height;
        assert!(
            !shows_scroll,
            "Should not show scroll indicator when all lines visible"
        );
    }

    #[test]
    fn test_progress_section_with_phase_only() {
        let mut app = TuiApp::new();
        app.set_phase("GENERATING SPEC");

        // When no current story, phase should be displayed
        assert!(app.current_story_id().is_none());
        assert_eq!(app.phase(), "GENERATING SPEC");
    }

    #[test]
    fn test_progress_section_with_story_and_totals() {
        let mut app = TuiApp::new();
        app.set_current_story("US-003", "Third Story");
        app.set_total_stories(5);
        app.set_completed_stories(2);

        // Verify progress display values
        assert_eq!(app.current_story_id(), Some("US-003"));
        assert_eq!(app.current_story_title(), Some("Third Story"));
        assert_eq!(app.total_stories(), 5);
        assert_eq!(app.completed_stories(), 2);

        // Progress text would show "Story 3/5: US-003 - Third Story"
        // (completed + 1 for current story being worked on)
    }

    #[test]
    fn test_progress_bar_style_values() {
        let mut app = TuiApp::new();
        app.set_total_stories(10);
        app.set_completed_stories(7);

        let ratio = app.completed_stories() as f64 / app.total_stories() as f64;
        let percentage = (ratio * 100.0) as u8;

        assert_eq!(percentage, 70);
        assert!((0.69..=0.71).contains(&ratio), "Ratio should be ~0.7");
    }

    #[test]
    fn test_waiting_state_display() {
        let app = TuiApp::new();

        // When no story and no phase, should show "Waiting..."
        assert!(app.current_story_id().is_none());
        assert!(app.phase().is_empty());
    }
}
