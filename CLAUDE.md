# CLAUDE.md - AI Agent Guide for autom8

## Project Overview

**autom8** is a Rust CLI tool for orchestrating Claude-powered development. It bridges the gap between product requirements (specs) and working code by driving Claude through iterative implementation of user stories.

**Core workflow:** Define features as structured user stories with acceptance criteria → autom8 orchestrates Claude to implement each story → Reviews for quality → Commits changes → Creates GitHub PRs.

## Quick Reference

```bash
# Build and run
cargo build
cargo run -- <command>

# Testing (must pass before PR)
cargo test --all-features
cargo fmt --check
cargo clippy -- -D warnings

# Common commands
cargo run -- run spec.json         # Run implementation from spec
cargo run -- status                # Check current run status
cargo run -- resume                # Resume interrupted run
cargo run -- list                  # List available specs

# Multi-session commands (worktree mode)
cargo run -- run --worktree        # Run in a dedicated worktree
cargo run -- status --all          # Show all sessions for project
cargo run -- resume --session ID   # Resume specific session
cargo run -- clean --worktrees     # Clean up sessions and worktrees
```

## Architecture

```
CLI (main.rs)
    ↓
Commands (commands/) - command handlers
    ↓
Runner (runner.rs) - orchestration loop
    ↓
State Machine (state.rs) - state management
    ↓
Display Adapter (display.rs) - abstraction
    ├→ CliDisplay (output/)
    └→ TuiDisplay (tui/)
    ↓
Domain Logic
    ├→ Spec (spec.rs) - user stories
    ├→ Config (config.rs) - settings
    ├→ Git (git.rs) - git operations
    ├→ GitHub (gh/) - PR management
    └→ Claude (claude/) - LLM integration

UI Layer (ui/) - standalone monitoring interfaces
    ├→ GUI (gui/) - Native desktop GUI using eframe/egui
    ├→ TUI (tui/) - Terminal UI using ratatui
    └→ Shared (shared/) - Common types and data loading
```

## Key Files and Modules

| File/Module | LOC | Purpose |
|-------------|-----|---------|
| `main.rs` | ~2,350 | CLI entry point, command parsing (clap) |
| `commands/` | ~4,800 | Command handlers (14 files: run, status, resume, clean, etc.) |
| `runner.rs` | ~3,900 | Main orchestration loop, state transitions, worktree context |
| `state.rs` | ~4,450 | State machine (12 states), session management, metadata |
| `worktree.rs` | ~1,650 | Git worktree operations, session ID generation |
| `claude/` | ~3,400 | Claude CLI integration (9 files: runner, stream, types, etc.) |
| `gh/` | ~1,850 | GitHub CLI integration (8 files: pr, detection, context, etc.) |
| `config.rs` | ~3,650 | TOML config, validation, defaults, worktree settings |
| `output/` | ~2,100 | CLI formatting (10 files: banner, messages, status, etc.) |
| `progress.rs` | ~2,200 | Spinners, progress bars, breadcrumbs |
| `spec.rs` | ~430 | Spec/UserStory structs, JSON serialization |
| `git.rs` | ~1,350 | Git command wrappers |
| `prompts.rs` | ~1,250 | Claude system prompts |
| `ui/` | ~24,050 | UI modules (see UI Directory Structure below) |

### UI Directory Structure

The `ui/` module provides standalone monitoring interfaces for viewing autom8 activity.

```
ui/
├── mod.rs              (~15 LOC)   - Module exports
├── gui/                (~17,830 LOC) - Native desktop GUI (eframe/egui)
│   ├── app.rs          (~14,730 LOC) - Main app, session cards, views, state
│   ├── components.rs   (~870 LOC)  - Reusable UI components, text truncation
│   ├── config.rs       (~600 LOC)  - Config tab types and edit logic
│   ├── modal.rs        (~590 LOC)  - Modal dialog component
│   ├── theme.rs        (~490 LOC)  - Colors, spacing, shadows, visuals
│   ├── typography.rs   (~290 LOC)  - Font sizes, weights, helpers
│   └── animation.rs    (~260 LOC)  - Decorative particle animations
├── tui/                (~4,700 LOC) - Terminal UI (ratatui/crossterm)
│   ├── app.rs          (~4,560 LOC) - Main app, views, event loop
│   └── views.rs        (~120 LOC)  - View enum definitions
└── shared/             (~1,520 LOC) - Framework-agnostic shared logic
    └── mod.rs          (~1,520 LOC) - Status types, data loading, formatting
```

**GUI (`gui/`)**: Native desktop application built with eframe/egui. Provides a warm, Claude-inspired aesthetic with session monitoring, run history, config editing, and project overview. Key features:
- Session cards with output display (clipped viewport, text truncation)
- Rising particle animations
- Modal dialogs for confirmations
- Responsive layout with sidebar navigation

**TUI (`tui/`)**: Terminal-based interface using ratatui. Keyboard-navigable dashboard showing active runs, project list, and run history.

**Shared (`shared/`)**: Common types and loading functions used by both GUI and TUI:
- `Status` enum for semantic state mapping
- `SessionData`, `ProjectData`, `RunHistoryEntry` types
- `load_ui_data()`, `load_run_history()` functions
- Time formatting utilities

## State Machine

The runner implements a deterministic state machine:

```
Idle → Initializing → PickingStory → RunningClaude → Reviewing → Correcting → Committing → CreatingPR → Completed
                                          ↑               ↓
                                          └───────────────┘
                                              (on issues)
```

States are defined in `state.rs` as `MachineState` enum. Transitions are explicit and persisted to the session's `state.json` file (see [Directory Structure](#directory-structure) in Worktree Architecture).

## Important Patterns

### 1. Display Adapter (Strategy Pattern)
All output goes through `DisplayAdapter` trait (`display.rs:30`). Never use `println!` directly in `runner.rs`.

```rust
// Good - uses adapter
display.phase_banner("Implementation", BannerColor::Green);

// Bad - direct output
println!("Implementation");
```

### 2. Progress Display Helper
Use `with_progress_display()` (`runner.rs`) for operations that need verbose/spinner handling:

```rust
with_progress_display(
    verbose,
    display,
    || VerboseTimer::new("operation"),
    || ClaudeSpinner::new(),
    || run_operation(),
    |result| map_to_outcome(result),
)
```

### 3. Loop Control
The main loop uses explicit `LoopAction` enum:
```rust
enum LoopAction { Continue, Break }
```

### 4. Error Handling
Use `thiserror` crate. Errors defined in `error.rs`:
```rust
#[derive(Debug, thiserror::Error)]
pub enum Autom8Error {
    #[error("Spec not found: {0}")]
    SpecNotFound(String),
    // ...
}
```

### 5. JSON Serialization
Specs use camelCase serialization:
```rust
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Spec { ... }
```

## Configuration

**Global config:** `~/.config/autom8/config.toml`
```toml
review = true            # Enable review step
commit = true            # Auto-commit changes
pull_request = true      # Auto-create PR
pull_request_draft = false  # Create PRs as drafts (only applies when pull_request = true)

# Worktree settings (for parallel sessions)
worktree = true               # Enable automatic worktree creation (default)
worktree_path_pattern = "{repo}-wt-{branch}"  # Worktree naming pattern
worktree_cleanup = false      # Remove worktrees after completion
```

**State persistence:** `~/.config/autom8/<project>/sessions/<session-id>/state.json`

## Claude Integration Notes

- Claude CLI subprocess spawning in `claude/runner.rs`
- Outputs newline-delimited JSON; parsed with stream parser (`claude/stream.rs`)
- Work summaries extracted from `<work-summary>` tags (max 500 chars) in `claude/utils.rs`
- Completion signals use `<promise>COMPLETE</promise>` tag
- Error info preserved with exit codes and stderr (`claude/types.rs`)

## Permission System

autom8 uses a minimal, phase-aware permission system that provides a safety net against accidental pushes to remote repositories while staying out of the way during normal operation.

### Design Philosophy

Within the project directory, almost everything is reversible via git. The only truly dangerous operation is pushing to a remote, which is hard to undo. The permission system is designed so that **normal usage should see zero prompts** - it only intervenes when Claude attempts `git push` during story implementation (which is unusual).

### Phase-Aware Permissions

Different phases of autom8's workflow have different permission needs. Permissions are configured in `claude/permissions.rs`:

| Phase | State | Permissions | Rationale |
|-------|-------|-------------|-----------|
| Story Implementation | `RunningClaude` | Block `git push` only | Full freedom for coding; safety net for remote |
| Review | `Reviewing` | Block `git push` only | Read-heavy, no special needs |
| Correction | `Correcting` | Block `git push` only | Same as story implementation |
| Commit | `Committing` | Block `git push` only | Needs `git add`, `git commit`; no push |
| PR Creation | `CreatingPR` | No restrictions | Needs `git push`, `gh pr *` |

All phases use:
- `--permission-mode acceptEdits` - auto-allow file operations
- `--allowedTools Bash` - allow most Bash commands
- `--disallowedTools "Bash(git push *)"` - block push (except PR phase)

### What Triggers Prompts

Prompts are **only** triggered when:
1. Claude attempts a blocked operation (currently just `git push` during non-PR phases)
2. The `--all-permissions` flag is **not** set

In practice, this should be rare since `git push` during story implementation is unusual. If triggered, autom8 shows:

```
⚠️  Claude wants to push to remote:

    git push origin feature-branch

This was blocked because it affects the remote repository.
Allow this operation? [y/N]:
```

The default response is "deny" (N) - blocked operations require explicit approval.

### CLI Flags

| Flag | Description |
|------|-------------|
| `--all-permissions` | Bypass phase-aware permissions and use `--dangerously-skip-permissions`. Useful for CI/CD or fully trusted environments where you want unattended execution with no restrictions. |

### Configuration

```toml
# In config.toml
all_permissions = false  # Default: use phase-aware permissions
                         # Set to true for CI/CD or fully trusted environments
```

### Key Files

| File | Purpose |
|------|---------|
| `claude/permissions.rs` | `ClaudePhase` enum and `build_permission_args()` function |
| `claude/control.rs` | Control protocol types for permission requests/responses |
| `display.rs` | `DisplayAdapter::prompt_permission()` for user interaction |
| `runner.rs` | Permission callback wiring in `handle_story_iteration()` |

### Edge Cases

- **Spec generation** (`claude/spec.rs`): Uses `--dangerously-skip-permissions` since it's a one-shot conversion task, not an implementation session.
- **PR review** (`claude/pr_review.rs`): Uses `--dangerously-skip-permissions` for analyzing and fixing PR comments.
- **TUI monitoring**: The TUI interface denies all permission requests by default since it's a read-only monitoring interface that doesn't support interactive prompts.

## Testing

Tests are in `#[cfg(test)]` blocks within modules. Focus areas:
- `runner.rs` - orchestration logic
- `spec.rs` - spec loading/saving

```bash
# Run all tests
cargo test --all-features

# Run specific module tests
cargo test spec::tests
cargo test runner::tests
```

## Code Style

- **Formatting:** `cargo fmt` (enforced by CI)
- **Linting:** `cargo clippy -- -D warnings` (zero warnings)
- **Imports:** Group std, external crates, then local modules
- **Error handling:** Return `Result<T, Autom8Error>`, use `?` operator
- **Documentation:** Doc comments for public API, inline for complex logic

## Common Tasks

### Adding a new CLI command
1. Add variant to `Commands` enum in `main.rs`
2. Create handler file in `commands/` (e.g., `commands/mycommand.rs`)
3. Add module declaration and re-export in `commands/mod.rs`
4. Call the handler from `main()` match statement

### Adding a new state
1. Add variant to `MachineState` enum in `state.rs`
2. Update state transition logic in `runner.rs`
3. Add display handling in `display.rs` and implementations

### Modifying Claude prompts
1. Edit prompts in `prompts.rs`
2. Prompts include detailed instructions and output format examples
3. Test with actual Claude CLI

### Adding display output
1. Add method to `DisplayAdapter` trait (`display.rs`)
2. Implement in `CliDisplay` (add function to appropriate `output/` submodule)
3. Implement in `TuiDisplay` (using `tui/app.rs`)

### Adding GUI features
1. Add shared types to `ui/shared/mod.rs` if needed by both GUI and TUI
2. Update `ui/gui/app.rs` for main application logic
3. Add reusable components to `ui/gui/components.rs`
4. Add colors/spacing to `ui/gui/theme.rs`
5. Add fonts to `ui/gui/typography.rs`
6. For modal dialogs, use `ui/gui/modal.rs`

## File Locations

- **Specs:** `~/.config/autom8/<project>/spec/spec-<feature>.json`
- **Session state:** `~/.config/autom8/<project>/sessions/<session-id>/state.json`
- **Session metadata:** `~/.config/autom8/<project>/sessions/<session-id>/metadata.json`
- **Archived runs:** `~/.config/autom8/<project>/runs/`
- **Global config:** `~/.config/autom8/config.toml`
- **Project config:** `~/.config/autom8/<project>/config.toml`
- **Worktrees:** `<repo-parent>/<repo>-wt-<branch>/` (when `worktree = true`)

## Dependencies (Key)

- `clap` - CLI parsing
- `serde`/`serde_json` - serialization
- `chrono` - datetime handling
- `thiserror` - error types
- `indicatif` - progress/spinners
- `ratatui`/`crossterm` - TUI
- `eframe`/`egui`/`egui_extras` - GUI framework
- `image` - image loading for GUI icons
- `toml` - config parsing

## CI/CD

- **test.yml:** Runs `cargo test --all-features`
- **lint.yml:** Runs `cargo fmt --check` and `cargo clippy`
- **release.yml:** Creates GitHub Release on tag

All checks must pass before merging PRs.

## Gotchas

1. **PR requires commit:** Config validation enforces `pull_request` requires `commit = true`
2. **TUI thread safety:** TUI uses `Arc<Mutex<TuiApp>>` for cross-thread access
3. **Output buffer limit:** TUI caps output at 1,000 lines to prevent memory growth
4. **GUI output clipping:** GUI uses hardware clipping (`painter.with_clip_rect()`) to constrain text within output display areas
5. **GUI output priority:** GUI follows TUI pattern: fresh live output > iteration snippet > status (5-second freshness threshold)
6. **State persistence:** Config snapshot saved at run start; resumed runs use same settings
7. **Branch handling:** Runner auto-creates/checkouts branches from spec's `branch_name`
8. **Branch conflicts:** Two sessions cannot use the same branch simultaneously; autom8 detects this and errors early
9. **Session identity:** In main repo, session ID is `"main"`; in worktrees, it's a hash of the path
10. **Stale sessions:** If a worktree is manually deleted, its session becomes "stale" and won't block new runs
11. **Project identity:** Project name is derived from git repo root, not CWD (ensures all worktrees share config)
12. **State persistence ordering:** In `run()` and `run_from_spec()`, state must NOT be persisted until after worktree context is determined. Saving state before `effective_state_manager` is known creates phantom sessions. Visual transitions (`print_state_transition()`) are fine; only `save()` calls must be deferred.

## Worktree Architecture

autom8 supports running multiple parallel sessions for the same project using git worktrees. This enables concurrent implementation of multiple features.

### Session Identity

Each session is identified by a deterministic session ID:
- **Main repository**: Uses the well-known ID `"main"`
- **Worktrees**: Uses an 8-character hex hash of the worktree's absolute path

Session IDs are filesystem-safe and stable (same path always produces the same ID).

### Directory Structure

```
~/.config/autom8/<project>/
├── config.toml                    # Project-level config
├── spec/                          # Spec files (shared across sessions)
│   └── spec-*.json
├── runs/                          # Archived runs (shared across sessions)
└── sessions/                      # Per-session state
    ├── main/                      # Main repo session
    │   ├── state.json             # Run state
    │   └── metadata.json          # Session metadata
    └── <session-id>/              # Worktree sessions
        ├── state.json
        └── metadata.json
```

### Worktree Modes

**When `worktree = true` (default):**
- Creates dedicated worktree at `<repo-parent>/<repo>-wt-<branch>/`
- Each worktree gets its own session with isolated state
- Multiple specs can run in parallel
- Worktrees can be auto-cleaned after successful completion (`worktree_cleanup = true`)

**When `worktree = false`:**
- Runs on current branch in main repository
- Single session per project
- State stored in `sessions/main/`

### Branch Conflict Detection

Before starting a run, autom8 checks if the branch is already in use:
1. Scans all session metadata files
2. Skips: own session, different branches, non-running sessions, stale sessions
3. Returns `BranchConflict` error if conflict found

A session is considered "stale" if its worktree path no longer exists.

### Key Files

| File | Purpose |
|------|---------|
| `worktree.rs` | Git worktree operations and session ID generation |
| `state.rs` | SessionMetadata struct, StateManager with session support |
| `config.rs` | `worktree`, `worktree_path_pattern`, `worktree_cleanup` fields |
| `runner.rs` | Worktree context setup and lifecycle management |

### Common Worktree Operations

```bash
# Run with worktree mode enabled (CLI override)
autom8 run --worktree --spec spec.json

# Check all sessions for current project
autom8 status --all

# Resume a specific session by ID
autom8 resume --session <id>

# List resumable sessions
autom8 resume --list

# Clean up completed sessions (preserves worktrees)
autom8 clean

# Clean up sessions and their worktrees
autom8 clean --worktrees

# Remove orphaned sessions (worktree deleted but state remains)
autom8 clean --orphaned
```

## Module Exports

`lib.rs` re-exports all public modules. When adding new modules:
1. Declare in `lib.rs`: `pub mod new_module;`
2. Re-export as needed: `pub use new_module::Type;`
