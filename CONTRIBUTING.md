# Contributing to autom8

Thank you for your interest in contributing to autom8!

## Development Environment Setup

### Prerequisites

- **Rust toolchain**: Install via [rustup](https://rustup.rs/)
  ```bash
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- **Git**: For version control
- **GitHub CLI** (`gh`): Optional, for testing PR creation features

### Getting Started

1. Clone the repository:
   ```bash
   git clone https://github.com/louisboilard/autom8.git
   cd autom8
   ```

2. Build the project:
   ```bash
   cargo build
   ```

3. Run the project:
   ```bash
   cargo run -- --help
   ```

## Running Tests

Run the full test suite:

```bash
cargo test --all-features
```

## Linting

Before submitting a PR, ensure your code passes all linting checks:

### Format Check

```bash
cargo fmt --check
```

To automatically fix formatting issues:

```bash
cargo fmt
```

### Clippy

```bash
cargo clippy -- -D warnings
```

## Pull Request Workflow

1. Create a feature branch from `main`
2. Make your changes
3. Ensure all checks pass locally:
   - `cargo test --all-features`
   - `cargo fmt --check`
   - `cargo clippy -- -D warnings`
4. Push your branch and open a PR against `main`

### CI Checks

All PRs must pass the following automated checks before merging:

| Check | Workflow | Command |
|-------|----------|---------|
| Tests | `.github/workflows/test.yml` | `cargo test --all-features` |
| Formatting | `.github/workflows/lint.yml` | `cargo fmt --check` |
| Lints | `.github/workflows/lint.yml` | `cargo clippy -- -D warnings` |

## Code Style

- Follow standard Rust conventions
- Use `cargo fmt` for consistent formatting
- Address all Clippy warnings
- Write tests for new functionality

## Versioning Strategy

This project follows [Semantic Versioning](https://semver.org/). Since we are pre-1.0, the rules are slightly relaxed:

### Version Number Format: `MAJOR.MINOR.PATCH`

- **MAJOR (0.x.x → 1.x.x)**: Reserved for when the project reaches production stability
- **MINOR (0.x.0)**: Bump for new features, significant changes, or breaking changes (pre-1.0)
- **PATCH (0.0.x)**: Bump for bug fixes and minor improvements

### When to Bump Versions

| Change Type | Version Bump | Example |
|-------------|--------------|---------|
| New CLI command | Minor | 0.2.0 → 0.3.0 |
| New feature | Minor | 0.2.0 → 0.3.0 |
| Breaking API change | Minor (pre-1.0) | 0.2.0 → 0.3.0 |
| Bug fix | Patch | 0.2.0 → 0.2.1 |
| Documentation only | None or Patch | - |
| Refactoring (no behavior change) | Patch | 0.2.0 → 0.2.1 |
| Dependency updates | Patch | 0.2.0 → 0.2.1 |

### Release Process

1. Update version in `Cargo.toml`
2. Update `CHANGELOG.md` with release notes
3. Create a git tag: `git tag v0.x.x`
4. Push the tag: `git push origin v0.x.x`

The release workflow will automatically build binaries and create a GitHub Release.
