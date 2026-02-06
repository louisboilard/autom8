# Contributing to autom8

Thank you for your interest in contributing to autom8!

Please note that this project is released with a [Contributor Code of Conduct](CODE_OF_CONDUCT.md). By participating in this project you agree to abide by its terms.

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

## Releasing

This section is primarily for maintainers who publish releases.

### Versioning

This project follows [Semantic Versioning](https://semver.org/). Version numbers are **manually managed** in `Cargo.toml`.

Since we are pre-1.0, the versioning rules are slightly relaxed:
- **MAJOR (0.x.x → 1.x.x)**: Reserved for production stability milestone
- **MINOR (0.x.0)**: New features, significant changes, or breaking changes
- **PATCH (0.0.x)**: Bug fixes, minor improvements, dependency updates

### Release Process

1. **Prepare the release**
   - Ensure all changes are merged to `main`
   - Update version in `Cargo.toml`
   - Update `CHANGELOG.md` with release notes
   - Commit: `git commit -am "Prepare v0.x.x release"`

2. **Create and push the tag**
   ```bash
   git tag v0.x.x
   git push origin v0.x.x
   ```

3. **CI takes over automatically**
   - The `release.yml` workflow triggers on version tags (`v*`)
   - Builds binaries for Linux (x86_64), macOS (x86_64, aarch64), and Windows (x86_64)
   - Creates a GitHub Release with the binaries attached
   - Publishes the crate to crates.io

4. **Verify the release**
   - Check the [GitHub Releases page](https://github.com/louisboilard/autom8/releases)
   - Verify the [crates.io page](https://crates.io/crates/autom8)
   - Test installation: `cargo install autom8`

### What CI Does Automatically

When you push a version tag (e.g., `v0.2.0`), the release workflow:

| Step | Description |
|------|-------------|
| Build | Compiles release binaries for 4 platform targets |
| Package | Renames binaries to `autom8-<target>` format |
| Release | Creates GitHub Release and uploads binaries |
| Publish | Runs `cargo publish` to push to crates.io |

### crates.io Token (Maintainers Only)

To publish to crates.io, the repository needs a `CARGO_REGISTRY_TOKEN` secret:

1. Log in to [crates.io](https://crates.io/) with your GitHub account
2. Go to [Account Settings → API Tokens](https://crates.io/settings/tokens)
3. Create a new token with scopes: `publish-new`, `publish-update`
4. Add the token as a repository secret named `CARGO_REGISTRY_TOKEN` in GitHub Settings → Secrets and variables → Actions
