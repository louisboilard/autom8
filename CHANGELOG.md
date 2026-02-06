# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.0] - 2026-02-06

### Added

- **Native desktop GUI**: Full-featured GUI built with eframe/egui featuring session monitoring, run history, config editing, and project overview with Claude-inspired aesthetic
- **Terminal UI (TUI)**: Keyboard-navigable dashboard using ratatui showing active runs, project list, and run history (`autom8 monitor` or `--tui` flag)
- **Git worktree support**: Run multiple parallel sessions for the same project using git worktrees (`--worktree` flag, enabled by default)
- **`improve` command**: Follow-up work on existing branches with Claude, carrying forward context from previous runs
- **TOML configuration**: Global and project-level config files (`~/.config/autom8/config.toml`) for persistent settings
- **Shell completions**: Tab completion for bash, zsh, fish, and PowerShell (`autom8 completions <shell>`)
- **Self-test mode**: `--self-test` flag for automated testing of the full run cycle
- **Draft PR option**: `pull_request_draft` config option to create PRs as drafts
- **Token usage tracking**: Display input/output token counts in CLI completion messages and GUI run details
- **PR template support**: Automatically uses repository's PR template when creating pull requests
- **Graceful signal handling**: Clean shutdown on Ctrl+C with state preservation
- **Heartbeat mechanism**: Robust status tracking for long-running sessions
- **GUI features**: Create Spec tab, config editor, modal dialogs, context menus, collapsible sections, and particle animations

### Changed

- **MSRV bumped to 1.88**: Minimum supported Rust version increased from 1.80
- **Security checks weekly**: Security audit workflow now runs on schedule instead of every push
- **Display adapter pattern**: All output goes through `DisplayAdapter` trait for TUI/CLI abstraction
- **Session management**: Sessions are now identified by deterministic IDs (main repo uses `"main"`, worktrees use path hash)

### Fixed

- **Phantom session bug**: Fixed state persistence ordering that created phantom sessions in worktree mode
- **GUI output clipping**: Hardware clipping constrains text within output display areas
- **Duration counter**: Fixed duration showing incorrectly on completed runs
- **Branch switch**: Fixed branch switching in markdown spec creation flow
- **Local timezone**: End times now display in local timezone with 12-hour format

## [0.2.0] - 2025-01-25

### Added

- **GitHub Actions CI/CD**: Test workflow (`test.yml`) and lint workflow (`lint.yml`) for automated checks on PRs
- **Automatic PR creation**: Branches are now automatically pushed and PRs created via `gh` CLI integration
- **New CLI commands**: `list`, `describe`, `projects` for viewing project status across the config directory
- **Global status view**: `autom8 status --all` shows status across all tracked projects
- **Spec file detection**: Automatically detects new spec files created during Claude sessions
- **Error panels**: Visual error display for Claude failures with stderr and exit code information
- **Progress bars**: Task progress visualization with spinner display
- **Phase banners and footers**: Visual framing for different execution phases
- **Contribution guidelines**: `CONTRIBUTING.md` with development setup and PR workflow
- **Comprehensive test coverage**: Added tests for `runner.rs` and `spec.rs` core modules
- **MSRV testing**: CI now tests against minimum supported Rust version (1.80)
- **Security auditing**: New `security.yml` workflow with cargo-audit for vulnerability detection
- **Cross-platform release**: Release workflow now builds Windows x86_64 binaries
- **Automated crates.io publishing**: Release workflow publishes to crates.io on tag push
- **Code of Conduct**: Added Contributor Covenant v2.1
- **MIT License**: Added LICENSE file for crates.io compliance

### Changed

- **Terminology update**: Renamed "PRD" to "spec" throughout the codebase
- **Simplified default command**: Running `autom8` without arguments now starts spec creation workflow
- **Config directory structure**: Unified spec storage under `~/.config/autom8/<project>/spec/`
- **Refactored runner.rs**: Extracted duplicate review/correct loop logic, broke up large methods, consolidated verbose/spinner display patterns
- **Test consolidation**: Removed ~53 redundant tests while maintaining behavior coverage

### Fixed

- **PR creation flow**: Fixed bug where PR creation failed because branch wasn't pushed to remote
- **JSON generation**: Added retry logic with 3 attempts for spec JSON generation

### Removed

- **Deprecated commands**: Removed `skill`, `history`, `archive`, and `new` commands in favor of simplified workflow

## [0.1.0] - Initial Release

### Added

- Core implementation loop for story-driven development
- Claude CLI integration for AI-powered code generation
- State machine for tracking run progress
- Review and correction loops for iterative improvement
- Spec file format (JSON and Markdown) for defining user stories
- Basic CLI with `run`, `status`, `resume`, `clean`, and `init` commands
