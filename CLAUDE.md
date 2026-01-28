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
cargo run -- run spec.json      # Run implementation from spec
cargo run -- status             # Check current run status
cargo run -- resume             # Resume interrupted run
cargo run -- list               # List available specs
```

## Architecture

```
CLI (main.rs)
    ↓
Runner (runner.rs) - orchestration loop
    ↓
State Machine (state.rs) - state management
    ↓
Display Adapter (display.rs) - abstraction
    ├→ CliDisplay (output.rs)
    └→ TuiDisplay (tui/)
    ↓
Domain Logic
    ├→ Spec (spec.rs) - user stories
    ├→ Config (config.rs) - settings
    ├→ Git (git.rs) - git operations
    ├→ GitHub (gh.rs) - PR management
    └→ Claude (claude.rs) - LLM integration
```

## Key Files and Modules

| File | LOC | Purpose |
|------|-----|---------|
| `main.rs` | ~1,900 | CLI entry point, command parsing (clap) |
| `runner.rs` | ~2,150 | Main orchestration loop, state transitions |
| `state.rs` | ~1,100 | State machine (12 states), persistence |
| `claude.rs` | ~3,500 | Claude CLI subprocess, JSON streaming |
| `gh.rs` | ~3,500 | GitHub CLI integration, PR operations |
| `config.rs` | ~3,100 | TOML config, validation, defaults |
| `output.rs` | ~2,500 | CLI formatting, colors, banners |
| `progress.rs` | ~2,200 | Spinners, progress bars, breadcrumbs |
| `display.rs` | ~940 | DisplayAdapter trait (strategy pattern) |
| `spec.rs` | ~430 | Spec/UserStory structs, JSON serialization |
| `git.rs` | ~810 | Git command wrappers |
| `prompts.rs` | ~1,100 | Claude system prompts |
| `tui/` | ~2,400 | Ratatui terminal UI |

## State Machine

The runner implements a deterministic state machine:

```
Idle → Initializing → PickingStory → RunningClaude → Reviewing → Correcting → Committing → CreatingPR → Completed
                                          ↑               ↓
                                          └───────────────┘
                                              (on issues)
```

States are defined in `state.rs` as `MachineState` enum. Transitions are explicit and persisted to `.autom8/state.json`.

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
review = true       # Enable review step
commit = true       # Auto-commit changes
pull_request = true # Auto-create PR
```

**State persistence:** `.autom8/state.json` (per-project)

## Claude Integration Notes

- Claude CLI subprocess spawning in `claude.rs`
- Outputs newline-delimited JSON; parsed with stream parser
- Work summaries extracted from `<work-summary>` tags (max 500 chars)
- Completion signals use `<promise>COMPLETE</promise>` tag
- Error info preserved with exit codes and stderr

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
2. Add handler in `main()` match statement
3. Add any new module functions needed

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
2. Implement in `CliDisplay` (using `output.rs`)
3. Implement in `TuiDisplay` (using `tui/app.rs`)

## File Locations

- **Specs:** `~/.config/autom8/<project>/spec/spec-<feature>.json`
- **State:** `.autom8/state.json`
- **Archived runs:** `.autom8/runs/`
- **Config:** `~/.config/autom8/config.toml`

## Dependencies (Key)

- `clap` - CLI parsing
- `serde`/`serde_json` - serialization
- `chrono` - datetime handling
- `thiserror` - error types
- `indicatif` - progress/spinners
- `ratatui`/`crossterm` - TUI
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
4. **State persistence:** Config snapshot saved at run start; resumed runs use same settings
5. **Branch handling:** Runner auto-creates/checkouts branches from spec's `branch_name`

## Module Exports

`lib.rs` re-exports all public modules. When adding new modules:
1. Declare in `lib.rs`: `pub mod new_module;`
2. Re-export as needed: `pub use new_module::Type;`
