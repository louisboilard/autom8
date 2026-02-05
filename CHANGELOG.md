# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
