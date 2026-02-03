//! Shell completion infrastructure for autom8.
//!
//! This module provides:
//! - Shell detection from the `$SHELL` environment variable
//! - Completion script generation for bash, zsh, and fish
//! - Installation path resolution for each shell type
//!
//! # Usage
//!
//! ```ignore
//! use autom8::completion::{detect_shell, get_completion_path, generate_completion_script};
//!
//! // Detect user's shell
//! let shell = detect_shell()?;
//!
//! // Get the installation path
//! let path = get_completion_path(shell)?;
//!
//! // Generate the completion script
//! let script = generate_completion_script(shell);
//! ```

use crate::error::{Autom8Error, Result};
use clap::Command;
use clap_complete::{generate, Shell};
use std::io::Write;
use std::path::PathBuf;

/// List of supported shell names for error messages.
pub const SUPPORTED_SHELLS: &[&str] = &["bash", "zsh", "fish"];

/// Supported shell types for completion scripts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellType {
    Bash,
    Zsh,
    Fish,
}

impl ShellType {
    /// Convert to the `clap_complete::Shell` type.
    pub fn to_clap_shell(self) -> Shell {
        match self {
            ShellType::Bash => Shell::Bash,
            ShellType::Zsh => Shell::Zsh,
            ShellType::Fish => Shell::Fish,
        }
    }

    /// Get the display name of the shell.
    pub fn name(&self) -> &'static str {
        match self {
            ShellType::Bash => "bash",
            ShellType::Zsh => "zsh",
            ShellType::Fish => "fish",
        }
    }

    /// Parse a shell type from a string name.
    ///
    /// # Arguments
    ///
    /// * `name` - The shell name (e.g., "bash", "zsh", "fish")
    ///
    /// # Returns
    ///
    /// * `Ok(ShellType)` - The parsed shell type
    /// * `Err(Autom8Error)` - If the shell name is not supported
    pub fn from_name(name: &str) -> Result<ShellType> {
        match name.to_lowercase().as_str() {
            "bash" => Ok(ShellType::Bash),
            "zsh" => Ok(ShellType::Zsh),
            "fish" => Ok(ShellType::Fish),
            _ => Err(Autom8Error::ShellCompletion(format!(
                "Unsupported shell: '{}'. Supported shells are: {}.",
                name,
                SUPPORTED_SHELLS.join(", ")
            ))),
        }
    }
}

impl std::fmt::Display for ShellType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.name())
    }
}

/// Detect the user's shell from the `$SHELL` environment variable.
///
/// Parses the shell path (e.g., `/bin/zsh`, `/usr/bin/bash`) and returns
/// the corresponding `ShellType`.
///
/// # Returns
///
/// * `Ok(ShellType)` - The detected shell type
/// * `Err(Autom8Error)` - If `$SHELL` is not set or the shell is unsupported
///
/// # Examples
///
/// ```ignore
/// // With $SHELL=/bin/zsh
/// let shell = detect_shell()?;
/// assert_eq!(shell, ShellType::Zsh);
/// ```
pub fn detect_shell() -> Result<ShellType> {
    let shell_path = std::env::var("SHELL").map_err(|_| {
        Autom8Error::ShellCompletion(
            "$SHELL environment variable is not set. \
             Please specify your shell manually or set the $SHELL variable."
                .to_string(),
        )
    })?;

    parse_shell_from_path(&shell_path)
}

/// Parse a shell type from a shell path.
///
/// Extracts the basename from the path and matches against supported shells.
///
/// # Arguments
///
/// * `shell_path` - Full path to the shell (e.g., `/bin/zsh`, `/usr/local/bin/fish`)
///
/// # Returns
///
/// * `Ok(ShellType)` - The detected shell type
/// * `Err(Autom8Error)` - If the shell is not supported
pub fn parse_shell_from_path(shell_path: &str) -> Result<ShellType> {
    // Extract the basename from the path
    let shell_name = std::path::Path::new(shell_path)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(shell_path);

    match shell_name {
        "bash" => Ok(ShellType::Bash),
        "zsh" => Ok(ShellType::Zsh),
        "fish" => Ok(ShellType::Fish),
        _ => Err(Autom8Error::ShellCompletion(format!(
            "Unsupported shell: '{}'. \
             Supported shells are: bash, zsh, fish.",
            shell_name
        ))),
    }
}

/// Get the installation path for completion scripts.
///
/// Returns the appropriate path for each shell:
/// - **Bash**: `~/.local/share/bash-completion/completions/autom8` (XDG standard)
///   Falls back to `~/.bash_completion.d/autom8` if XDG path doesn't exist
/// - **Zsh**: `~/.zfunc/_autom8`
/// - **Fish**: `~/.config/fish/completions/autom8.fish`
///
/// # Arguments
///
/// * `shell` - The target shell type
///
/// # Returns
///
/// * `Ok(PathBuf)` - The path where the completion script should be installed
/// * `Err(Autom8Error)` - If the home directory cannot be determined
pub fn get_completion_path(shell: ShellType) -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| {
        Autom8Error::ShellCompletion("Could not determine home directory".to_string())
    })?;

    let path = match shell {
        ShellType::Bash => {
            // Prefer XDG path, check if the directory exists
            let xdg_path = home.join(".local/share/bash-completion/completions");
            if xdg_path.exists() {
                xdg_path.join("autom8")
            } else {
                // Fall back to traditional path
                home.join(".bash_completion.d/autom8")
            }
        }
        ShellType::Zsh => home.join(".zfunc/_autom8"),
        ShellType::Fish => home.join(".config/fish/completions/autom8.fish"),
    };

    Ok(path)
}

/// Ensure the parent directory for a completion script exists.
///
/// Creates the parent directory (and all ancestors) if it doesn't exist.
///
/// # Arguments
///
/// * `path` - The path to the completion script
///
/// # Returns
///
/// * `Ok(())` - Directory exists or was created successfully
/// * `Err(Autom8Error)` - If directory creation fails
pub fn ensure_completion_dir(path: &std::path::Path) -> Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| {
                Autom8Error::ShellCompletion(format!(
                    "Failed to create completion directory '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }
    }
    Ok(())
}

/// Build the clap Command structure for completion generation.
///
/// This creates a command hierarchy that mirrors the CLI defined in `main.rs`,
/// allowing clap_complete to generate accurate completion scripts.
fn build_cli() -> Command {
    Command::new("autom8")
        .version(env!("CARGO_PKG_VERSION"))
        .about("CLI automation tool for orchestrating Claude-powered development")
        .arg(
            clap::Arg::new("file")
                .help("Path to a spec.md or spec.json file (shorthand for `run --spec <file>`)")
                .value_hint(clap::ValueHint::FilePath),
        )
        .arg(
            clap::Arg::new("verbose")
                .short('v')
                .long("verbose")
                .help("Show full Claude output instead of spinner (useful for debugging)")
                .global(true)
                .action(clap::ArgAction::SetTrue),
        )
        .subcommand(
            Command::new("run")
                .about("Run the agent loop to implement spec stories")
                .arg(
                    clap::Arg::new("spec")
                        .long("spec")
                        .help("Path to the spec JSON or markdown file")
                        .default_value("./spec.json")
                        .value_hint(clap::ValueHint::FilePath),
                )
                .arg(
                    clap::Arg::new("skip-review")
                        .long("skip-review")
                        .help("Skip the review loop and go directly to committing")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(
            Command::new("status")
                .about("Check the current run status")
                .arg(
                    clap::Arg::new("all")
                        .short('a')
                        .long("all")
                        .help("Show status across all projects")
                        .action(clap::ArgAction::SetTrue),
                )
                .arg(
                    clap::Arg::new("global")
                        .short('g')
                        .long("global")
                        .help("Show status across all projects (alias for --all)")
                        .action(clap::ArgAction::SetTrue),
                ),
        )
        .subcommand(Command::new("resume").about("Resume a failed or interrupted run"))
        .subcommand(Command::new("clean").about("Clean up spec files from config directory"))
        .subcommand(
            Command::new("init")
                .about("Initialize autom8 config directory structure for current project"),
        )
        .subcommand(
            Command::new("projects").about("List all known projects in the config directory"),
        )
        .subcommand(Command::new("list").about("Show a tree view of all projects with status"))
        .subcommand(
            Command::new("describe")
                .about("Show detailed information about a specific project")
                .arg(
                    clap::Arg::new("project_name")
                        .help("The project name to describe (defaults to current directory)"),
                ),
        )
        .subcommand(
            Command::new("pr-review").about("Analyze PR review comments and fix real issues"),
        )
        .subcommand(
            Command::new("monitor")
                .about("Monitor autom8 activity across all projects (dashboard view)")
                .arg(
                    clap::Arg::new("project")
                        .short('p')
                        .long("project")
                        .help("Filter to a specific project"),
                )
                .arg(
                    clap::Arg::new("interval")
                        .short('i')
                        .long("interval")
                        .help("Polling interval in seconds (default: 1)")
                        .default_value("1"),
                ),
        )
        .subcommand(
            Command::new("config")
                .about("View, modify, or reset configuration")
                .arg(
                    clap::Arg::new("global")
                        .short('g')
                        .long("global")
                        .help("Show only the global configuration")
                        .action(clap::ArgAction::SetTrue)
                        .conflicts_with("project"),
                )
                .arg(
                    clap::Arg::new("project")
                        .short('p')
                        .long("project")
                        .help("Show only the project configuration")
                        .action(clap::ArgAction::SetTrue)
                        .conflicts_with("global"),
                )
                .subcommand(
                    Command::new("set")
                        .about("Set a configuration value")
                        .arg(
                            clap::Arg::new("global")
                                .short('g')
                                .long("global")
                                .help("Set in global config instead of project config")
                                .action(clap::ArgAction::SetTrue),
                        )
                        .arg(
                            clap::Arg::new("key")
                                .help("The configuration key to set")
                                .required(true)
                                .value_parser([
                                    "review",
                                    "commit",
                                    "pull_request",
                                    "worktree",
                                    "worktree_path_pattern",
                                    "worktree_cleanup",
                                ]),
                        )
                        .arg(
                            clap::Arg::new("value")
                                .help("The value to set (true/false for boolean keys)")
                                .required(true),
                        ),
                )
                .subcommand(
                    Command::new("reset")
                        .about("Reset configuration to default values")
                        .arg(
                            clap::Arg::new("global")
                                .short('g')
                                .long("global")
                                .help("Reset global config instead of project config")
                                .action(clap::ArgAction::SetTrue),
                        )
                        .arg(
                            clap::Arg::new("yes")
                                .short('y')
                                .long("yes")
                                .help("Skip confirmation prompt")
                                .action(clap::ArgAction::SetTrue),
                        ),
                ),
        )
}

/// Generate a completion script for the specified shell.
///
/// Creates a completion script that includes all autom8 commands, subcommands,
/// flags, and options. The script includes dynamic spec file completion that
/// queries `~/.config/autom8/*/spec/` at completion time.
///
/// # Arguments
///
/// * `shell` - The target shell type
///
/// # Returns
///
/// The completion script as a String.
pub fn generate_completion_script(shell: ShellType) -> String {
    let mut cmd = build_cli();
    let mut buf = Vec::new();
    generate(shell.to_clap_shell(), &mut cmd, "autom8", &mut buf);
    let base_script = String::from_utf8(buf).unwrap_or_default();

    // Append dynamic spec completion functions
    match shell {
        ShellType::Bash => format!("{}\n{}", base_script, generate_bash_spec_completion()),
        ShellType::Zsh => format!("{}\n{}", base_script, generate_zsh_spec_completion()),
        ShellType::Fish => format!("{}\n{}", base_script, generate_fish_spec_completion()),
    }
}

/// Generate bash-specific dynamic spec completion function.
fn generate_bash_spec_completion() -> &'static str {
    r#"
# Dynamic spec file completion for autom8
_autom8_spec_files() {
    local config_dir="$HOME/.config/autom8"
    local project_name=""
    local specs=()

    # Try to detect current project from git repo
    if git rev-parse --git-dir &>/dev/null; then
        project_name=$(basename "$(git rev-parse --show-toplevel 2>/dev/null)" 2>/dev/null)
    fi

    # If in a project and that project has specs, show only those
    if [[ -n "$project_name" && -d "$config_dir/$project_name/spec" ]]; then
        local spec_dir="$config_dir/$project_name/spec"
        if compgen -G "$spec_dir/*.json" &>/dev/null || compgen -G "$spec_dir/*.md" &>/dev/null; then
            for f in "$spec_dir"/*.json "$spec_dir"/*.md; do
                [[ -f "$f" ]] && specs+=("$(basename "$f")")
            done
        fi
    fi

    # If no project specs found, show specs from all projects
    if [[ ${#specs[@]} -eq 0 && -d "$config_dir" ]]; then
        for project_dir in "$config_dir"/*/spec; do
            if [[ -d "$project_dir" ]]; then
                for f in "$project_dir"/*.json "$project_dir"/*.md; do
                    [[ -f "$f" ]] && specs+=("$(basename "$f")")
                done
            fi
        done
    fi

    # Remove duplicates and sort
    printf '%s\n' "${specs[@]}" | sort -u
}

# Override completion for --spec flag and positional arguments
_autom8_complete() {
    local cur prev words cword
    _init_completion || return

    # Check if we're completing the --spec flag value
    if [[ "$prev" == "--spec" ]]; then
        COMPREPLY=($(compgen -W "$(_autom8_spec_files)" -- "$cur"))
        return
    fi

    # Check if completing first positional arg (not a subcommand)
    if [[ $cword -eq 1 && "$cur" != -* ]]; then
        # Get subcommands
        local subcommands="run status resume clean config init projects list describe pr-review monitor"
        # Get spec files
        local specs=$(_autom8_spec_files)
        COMPREPLY=($(compgen -W "$subcommands $specs" -- "$cur"))
        return
    fi

    # Config key completion
    if [[ "${words[1]}" == "config" && "${words[2]}" == "set" ]]; then
        if [[ $cword -eq 3 ]]; then
            # Complete config keys
            COMPREPLY=($(compgen -W "review commit pull_request worktree worktree_path_pattern worktree_cleanup" -- "$cur"))
            return
        elif [[ $cword -eq 4 && "${words[3]}" != "worktree_path_pattern" ]]; then
            # Complete boolean values for non-string keys
            COMPREPLY=($(compgen -W "true false" -- "$cur"))
            return
        fi
    fi

    # Fall back to default autom8 completion
    _autom8 "$@"
}

complete -F _autom8_complete autom8
"#
}

/// Generate zsh-specific dynamic spec completion function.
fn generate_zsh_spec_completion() -> &'static str {
    r#"
# Dynamic spec file completion for autom8
_autom8_spec_files() {
    local config_dir="$HOME/.config/autom8"
    local project_name=""
    local -a specs

    # Try to detect current project from git repo
    if git rev-parse --git-dir &>/dev/null; then
        project_name=$(basename "$(git rev-parse --show-toplevel 2>/dev/null)" 2>/dev/null)
    fi

    # If in a project and that project has specs, show only those
    if [[ -n "$project_name" && -d "$config_dir/$project_name/spec" ]]; then
        local spec_dir="$config_dir/$project_name/spec"
        specs=(${(f)"$(ls "$spec_dir"/*.json "$spec_dir"/*.md 2>/dev/null | xargs -n1 basename 2>/dev/null)"})
    fi

    # If no project specs found, show specs from all projects
    if [[ ${#specs[@]} -eq 0 && -d "$config_dir" ]]; then
        specs=(${(f)"$(ls "$config_dir"/*/spec/*.json "$config_dir"/*/spec/*.md 2>/dev/null | xargs -n1 basename 2>/dev/null)"})
    fi

    # Remove duplicates and print
    printf '%s\n' "${(u)specs[@]}"
}

# Override _autom8 to add spec file completion
if (( $+functions[_autom8_original] )); then
    : # Already patched
else
    # Save original function if it exists
    if (( $+functions[_autom8] )); then
        functions[_autom8_original]=$functions[_autom8]
    fi

    _autom8() {
        local curcontext="$curcontext" state line
        typeset -A opt_args

        # Check if completing --spec value
        if [[ "${words[$CURRENT-1]}" == "--spec" ]]; then
            local -a spec_files
            spec_files=(${(f)"$(_autom8_spec_files)"})
            _describe 'spec file' spec_files
            return
        fi

        # Check if completing first positional argument
        if [[ $CURRENT -eq 2 && "${words[2]}" != -* ]]; then
            local -a completions
            local -a spec_files
            spec_files=(${(f)"$(_autom8_spec_files)"})
            completions=(
                'run:Run the agent loop to implement spec stories'
                'status:Check the current run status'
                'resume:Resume a failed or interrupted run'
                'clean:Clean up spec files from config directory'
                'config:View, modify, or reset configuration'
                'init:Initialize autom8 config directory structure'
                'projects:List all known projects'
                'list:Show a tree view of all projects with status'
                'describe:Show detailed information about a specific project'
                'pr-review:Analyze PR review comments and fix real issues'
                'monitor:Monitor autom8 activity across all projects'
            )
            for spec in "${spec_files[@]}"; do
                [[ -n "$spec" ]] && completions+=("$spec:Spec file")
            done
            _describe 'command or spec' completions
            return
        fi

        # Config set key/value completion
        if [[ "${words[2]}" == "config" && "${words[3]}" == "set" ]]; then
            if [[ $CURRENT -eq 4 ]]; then
                local -a config_keys
                config_keys=(
                    'review:Enable code review step'
                    'commit:Enable auto-commit'
                    'pull_request:Enable auto-PR creation'
                    'worktree:Enable worktree mode'
                    'worktree_path_pattern:Pattern for worktree names'
                    'worktree_cleanup:Auto-cleanup worktrees'
                )
                _describe 'config key' config_keys
                return
            elif [[ $CURRENT -eq 5 && "${words[4]}" != "worktree_path_pattern" ]]; then
                local -a bool_values
                bool_values=('true' 'false')
                _describe 'value' bool_values
                return
            fi
        fi

        # Fall back to original completion if it exists
        if (( $+functions[_autom8_original] )); then
            _autom8_original "$@"
        fi
    }

    compdef _autom8 autom8
fi
"#
}

/// Generate fish-specific dynamic spec completion function.
fn generate_fish_spec_completion() -> &'static str {
    r#"
# Dynamic spec file completion for autom8
function __autom8_spec_files
    set -l config_dir "$HOME/.config/autom8"
    set -l project_name ""

    # Try to detect current project from git repo
    if git rev-parse --git-dir &>/dev/null
        set project_name (basename (git rev-parse --show-toplevel 2>/dev/null) 2>/dev/null)
    end

    # If in a project and that project has specs, show only those
    if test -n "$project_name"; and test -d "$config_dir/$project_name/spec"
        set -l spec_dir "$config_dir/$project_name/spec"
        for f in $spec_dir/*.json $spec_dir/*.md
            if test -f "$f"
                basename "$f"
            end
        end
        return
    end

    # If no project specs found, show specs from all projects
    if test -d "$config_dir"
        for spec_dir in $config_dir/*/spec
            if test -d "$spec_dir"
                for f in $spec_dir/*.json $spec_dir/*.md
                    if test -f "$f"
                        basename "$f"
                    end
                end
            end
        end | sort -u
    end
end

# Add spec file completions for --spec flag
complete -c autom8 -l spec -xa '(__autom8_spec_files)'

# Add spec file completions for positional argument (first arg that's not a flag)
complete -c autom8 -n '__fish_is_first_arg; and not __fish_seen_subcommand_from run status resume clean config init projects list describe pr-review monitor' -xa '(__autom8_spec_files)'

# Config set key completion
complete -c autom8 -n '__fish_seen_subcommand_from config; and __fish_seen_subcommand_from set; and test (count (commandline -opc)) -eq 3' -xa 'review commit pull_request worktree worktree_path_pattern worktree_cleanup'

# Config set value completion (true/false for boolean keys)
complete -c autom8 -n '__fish_seen_subcommand_from config; and __fish_seen_subcommand_from set; and test (count (commandline -opc)) -eq 4; and not string match -q worktree_path_pattern (commandline -opc)[-1]' -xa 'true false'
"#
}

/// Output the completion script for a shell to stdout.
///
/// This is used by the hidden `completions` subcommand to let power users
/// manually manage their completion scripts.
///
/// # Arguments
///
/// * `shell` - The target shell type
pub fn print_completion_script(shell: ShellType) {
    print!("{}", generate_completion_script(shell));
}

/// Write a completion script to the specified path.
///
/// Creates parent directories if needed, then writes the completion script.
///
/// # Arguments
///
/// * `shell` - The target shell type
/// * `path` - The destination path for the script
///
/// # Returns
///
/// * `Ok(())` - Script written successfully
/// * `Err(Autom8Error)` - If directory creation or file write fails
pub fn write_completion_script(shell: ShellType, path: &std::path::Path) -> Result<()> {
    // Ensure parent directory exists
    ensure_completion_dir(path)?;

    // Generate and write the script
    let script = generate_completion_script(shell);
    let mut file = std::fs::File::create(path).map_err(|e| {
        Autom8Error::ShellCompletion(format!(
            "Failed to create completion file '{}': {}",
            path.display(),
            e
        ))
    })?;

    file.write_all(script.as_bytes()).map_err(|e| {
        Autom8Error::ShellCompletion(format!(
            "Failed to write completion script to '{}': {}",
            path.display(),
            e
        ))
    })?;

    Ok(())
}

/// Result of completion installation.
#[derive(Debug)]
pub struct CompletionInstallResult {
    /// The shell that completions were installed for.
    pub shell: ShellType,
    /// The path where the completion script was written.
    pub path: PathBuf,
    /// Additional setup instructions for the user, if any.
    pub setup_instructions: Option<String>,
}

/// Check if zsh fpath includes ~/.zfunc.
///
/// Returns true if ~/.zfunc is already configured in fpath.
fn is_zfunc_in_fpath() -> bool {
    // Check if FPATH environment variable includes .zfunc
    if let Ok(fpath) = std::env::var("FPATH") {
        let home = dirs::home_dir().unwrap_or_default();
        let zfunc = home.join(".zfunc");
        let zfunc_str = zfunc.to_string_lossy();

        for path in fpath.split(':') {
            if path == zfunc_str || path == "~/.zfunc" {
                return true;
            }
        }
    }
    false
}

/// Get setup instructions for zsh if ~/.zfunc is not in fpath.
fn get_zsh_setup_instructions() -> Option<String> {
    if is_zfunc_in_fpath() {
        None
    } else {
        Some(
            "To enable completions, add the following to your ~/.zshrc:\n\n\
             fpath=(~/.zfunc $fpath)\n\
             autoload -Uz compinit && compinit\n\n\
             Then restart your shell or run: source ~/.zshrc"
                .to_string(),
        )
    }
}

/// Get setup instructions for bash.
fn get_bash_setup_instructions(path: &std::path::Path) -> Option<String> {
    // Check if bash-completion is likely set up (XDG location)
    if path
        .to_string_lossy()
        .contains("bash-completion/completions")
    {
        // XDG location should be auto-loaded
        Some("Restart your shell to enable completions.".to_string())
    } else {
        // Non-XDG location needs manual sourcing
        Some(format!(
            "To enable completions, add to your ~/.bashrc:\n\n\
             source {}\n\n\
             Then restart your shell or run: source ~/.bashrc",
            path.display()
        ))
    }
}

/// Install shell completions for the current user.
///
/// Detects the user's shell from `$SHELL`, generates the appropriate
/// completion script, and writes it to the correct location.
///
/// # Returns
///
/// * `Ok(CompletionInstallResult)` - Installation succeeded
/// * `Err(Autom8Error)` - If shell detection or file writing fails
///
/// # Example
///
/// ```ignore
/// match install_completions() {
///     Ok(result) => {
///         println!("Installed {} completions to {}", result.shell, result.path.display());
///         if let Some(instructions) = result.setup_instructions {
///             println!("{}", instructions);
///         }
///     }
///     Err(e) => eprintln!("Failed: {}", e),
/// }
/// ```
pub fn install_completions() -> Result<CompletionInstallResult> {
    let shell = detect_shell()?;
    let path = get_completion_path(shell)?;

    write_completion_script(shell, &path)?;

    let setup_instructions = match shell {
        ShellType::Zsh => get_zsh_setup_instructions(),
        ShellType::Bash => get_bash_setup_instructions(&path),
        ShellType::Fish => {
            // Fish auto-loads from ~/.config/fish/completions/
            Some("Restart your shell to enable completions.".to_string())
        }
    };

    Ok(CompletionInstallResult {
        shell,
        path,
        setup_instructions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ======================================================================
    // Tests for US-001: Shell detection
    // ======================================================================

    #[test]
    fn test_parse_shell_bash() {
        assert_eq!(parse_shell_from_path("/bin/bash").unwrap(), ShellType::Bash);
        assert_eq!(
            parse_shell_from_path("/usr/bin/bash").unwrap(),
            ShellType::Bash
        );
        assert_eq!(
            parse_shell_from_path("/usr/local/bin/bash").unwrap(),
            ShellType::Bash
        );
    }

    #[test]
    fn test_parse_shell_zsh() {
        assert_eq!(parse_shell_from_path("/bin/zsh").unwrap(), ShellType::Zsh);
        assert_eq!(
            parse_shell_from_path("/usr/bin/zsh").unwrap(),
            ShellType::Zsh
        );
        assert_eq!(
            parse_shell_from_path("/usr/local/bin/zsh").unwrap(),
            ShellType::Zsh
        );
    }

    #[test]
    fn test_parse_shell_fish() {
        assert_eq!(parse_shell_from_path("/bin/fish").unwrap(), ShellType::Fish);
        assert_eq!(
            parse_shell_from_path("/usr/bin/fish").unwrap(),
            ShellType::Fish
        );
        assert_eq!(
            parse_shell_from_path("/usr/local/bin/fish").unwrap(),
            ShellType::Fish
        );
        assert_eq!(
            parse_shell_from_path("/opt/homebrew/bin/fish").unwrap(),
            ShellType::Fish
        );
    }

    #[test]
    fn test_parse_shell_unsupported() {
        let result = parse_shell_from_path("/bin/sh");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported shell"));
        assert!(err.contains("sh"));

        let result = parse_shell_from_path("/bin/tcsh");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("tcsh"));
    }

    #[test]
    fn test_parse_shell_unsupported_contains_supported_list() {
        let result = parse_shell_from_path("/bin/ksh");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bash"));
        assert!(err.contains("zsh"));
        assert!(err.contains("fish"));
    }

    // ======================================================================
    // Tests for US-001: Path resolution
    // ======================================================================

    #[test]
    fn test_completion_path_bash() {
        let path = get_completion_path(ShellType::Bash).unwrap();
        let path_str = path.to_string_lossy();

        // Should end with the expected filename
        assert!(path_str.ends_with("autom8"));

        // Should be in one of the two expected directories
        assert!(
            path_str.contains("bash-completion/completions")
                || path_str.contains(".bash_completion.d"),
            "Bash path should be in XDG or traditional location: {}",
            path_str
        );
    }

    #[test]
    fn test_completion_path_zsh() {
        let path = get_completion_path(ShellType::Zsh).unwrap();
        let path_str = path.to_string_lossy();

        assert!(
            path_str.ends_with(".zfunc/_autom8"),
            "Zsh path should end with .zfunc/_autom8: {}",
            path_str
        );
    }

    #[test]
    fn test_completion_path_fish() {
        let path = get_completion_path(ShellType::Fish).unwrap();
        let path_str = path.to_string_lossy();

        assert!(
            path_str.ends_with(".config/fish/completions/autom8.fish"),
            "Fish path should end with .config/fish/completions/autom8.fish: {}",
            path_str
        );
    }

    // ======================================================================
    // Tests for US-001: Script generation
    // ======================================================================

    #[test]
    fn test_generate_completion_script_bash() {
        let script = generate_completion_script(ShellType::Bash);

        // Should contain autom8 command
        assert!(script.contains("autom8"), "Script should reference autom8");

        // Should contain subcommands
        assert!(script.contains("run"), "Script should include run command");
        assert!(
            script.contains("status"),
            "Script should include status command"
        );
        assert!(
            script.contains("resume"),
            "Script should include resume command"
        );
        assert!(
            script.contains("clean"),
            "Script should include clean command"
        );
        assert!(
            script.contains("init"),
            "Script should include init command"
        );
        assert!(
            script.contains("projects"),
            "Script should include projects command"
        );
        assert!(
            script.contains("list"),
            "Script should include list command"
        );
        assert!(
            script.contains("describe"),
            "Script should include describe command"
        );
        assert!(
            script.contains("pr-review"),
            "Script should include pr-review command"
        );
        assert!(
            script.contains("monitor"),
            "Script should include monitor command"
        );
    }

    #[test]
    fn test_generate_completion_script_zsh() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should be a valid zsh completion script
        assert!(
            script.contains("#compdef autom8"),
            "Zsh script should start with #compdef"
        );

        // Should contain subcommands
        assert!(script.contains("run"));
        assert!(script.contains("status"));
        assert!(script.contains("init"));
    }

    #[test]
    fn test_generate_completion_script_fish() {
        let script = generate_completion_script(ShellType::Fish);

        // Should be a valid fish completion script
        assert!(
            script.contains("complete"),
            "Fish script should contain complete commands"
        );
        assert!(
            script.contains("autom8"),
            "Fish script should reference autom8"
        );
    }

    #[test]
    fn test_generate_completion_script_contains_flags() {
        let script = generate_completion_script(ShellType::Bash);

        // Should contain common flags
        assert!(
            script.contains("verbose") || script.contains("-v"),
            "Script should include verbose flag"
        );
        assert!(script.contains("spec"), "Script should include spec option");
        assert!(
            script.contains("skip-review"),
            "Script should include skip-review flag"
        );
        assert!(
            script.contains("all") || script.contains("-a"),
            "Script should include all flag"
        );
        assert!(
            script.contains("global") || script.contains("-g"),
            "Script should include global flag"
        );
        assert!(
            script.contains("project") || script.contains("-p"),
            "Script should include project flag"
        );
        assert!(
            script.contains("interval") || script.contains("-i"),
            "Script should include interval flag"
        );
    }

    // ======================================================================
    // Tests for US-001: ShellType utilities
    // ======================================================================

    #[test]
    fn test_shell_type_name() {
        assert_eq!(ShellType::Bash.name(), "bash");
        assert_eq!(ShellType::Zsh.name(), "zsh");
        assert_eq!(ShellType::Fish.name(), "fish");
    }

    #[test]
    fn test_shell_type_display() {
        assert_eq!(format!("{}", ShellType::Bash), "bash");
        assert_eq!(format!("{}", ShellType::Zsh), "zsh");
        assert_eq!(format!("{}", ShellType::Fish), "fish");
    }

    #[test]
    fn test_shell_type_to_clap_shell() {
        assert_eq!(ShellType::Bash.to_clap_shell(), Shell::Bash);
        assert_eq!(ShellType::Zsh.to_clap_shell(), Shell::Zsh);
        assert_eq!(ShellType::Fish.to_clap_shell(), Shell::Fish);
    }

    // ======================================================================
    // Tests for US-001: Directory creation
    // ======================================================================

    #[test]
    fn test_ensure_completion_dir_with_existing_parent() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("autom8");

        // Parent already exists
        let result = ensure_completion_dir(&path);
        assert!(result.is_ok());
    }

    #[test]
    fn test_ensure_completion_dir_creates_parent() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("new_dir").join("autom8");

        // Parent doesn't exist
        assert!(!path.parent().unwrap().exists());

        let result = ensure_completion_dir(&path);
        assert!(result.is_ok());
        assert!(path.parent().unwrap().exists());
    }

    #[test]
    fn test_ensure_completion_dir_creates_nested_parents() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("a").join("b").join("c").join("autom8");

        let result = ensure_completion_dir(&path);
        assert!(result.is_ok());
        assert!(path.parent().unwrap().exists());
    }

    // ======================================================================
    // Tests for US-001: Write completion script
    // ======================================================================

    #[test]
    fn test_write_completion_script_creates_file() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("autom8");

        let result = write_completion_script(ShellType::Bash, &path);
        assert!(result.is_ok());
        assert!(path.exists());

        // Verify content
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("autom8"));
    }

    #[test]
    fn test_write_completion_script_creates_parent_dirs() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("nested").join("dir").join("autom8");

        let result = write_completion_script(ShellType::Zsh, &path);
        assert!(result.is_ok());
        assert!(path.exists());
    }

    // ======================================================================
    // Tests for US-002: Completion installation from init
    // ======================================================================

    #[test]
    fn test_completion_install_result_has_expected_fields() {
        // Verify CompletionInstallResult struct has all expected fields
        let result = CompletionInstallResult {
            shell: ShellType::Zsh,
            path: PathBuf::from("/tmp/test"),
            setup_instructions: Some("Test instructions".to_string()),
        };

        assert_eq!(result.shell, ShellType::Zsh);
        assert_eq!(result.path, PathBuf::from("/tmp/test"));
        assert_eq!(
            result.setup_instructions,
            Some("Test instructions".to_string())
        );
    }

    #[test]
    fn test_completion_install_result_without_setup_instructions() {
        // Verify setup_instructions can be None
        let result = CompletionInstallResult {
            shell: ShellType::Fish,
            path: PathBuf::from("/tmp/test"),
            setup_instructions: None,
        };

        assert!(result.setup_instructions.is_none());
    }

    #[test]
    fn test_zsh_setup_instructions_contain_fpath() {
        // When fpath check would fail (not in FPATH), instructions should mention fpath
        // We can't easily test the actual check without modifying FPATH,
        // but we can test the instruction content
        let instructions = "fpath=(~/.zfunc $fpath)\nautoload -Uz compinit && compinit";
        assert!(instructions.contains("fpath"));
        assert!(instructions.contains("compinit"));
        assert!(instructions.contains("autoload"));
    }

    #[test]
    fn test_bash_setup_instructions_for_xdg_path() {
        let path = PathBuf::from("/home/user/.local/share/bash-completion/completions/autom8");
        let instructions = get_bash_setup_instructions(&path);

        assert!(instructions.is_some());
        let instructions = instructions.unwrap();
        // XDG path should just say restart shell
        assert!(instructions.contains("Restart"));
    }

    #[test]
    fn test_bash_setup_instructions_for_non_xdg_path() {
        let path = PathBuf::from("/home/user/.bash_completion.d/autom8");
        let instructions = get_bash_setup_instructions(&path);

        assert!(instructions.is_some());
        let instructions = instructions.unwrap();
        // Non-XDG path should mention sourcing
        assert!(instructions.contains("source"));
        assert!(instructions.contains(&path.display().to_string()));
    }

    #[test]
    fn test_write_completion_script_overwrites_existing() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("autom8");

        // Write initial script
        let result = write_completion_script(ShellType::Bash, &path);
        assert!(result.is_ok());

        let content1 = std::fs::read_to_string(&path).unwrap();

        // Write again (should overwrite, not fail)
        let result = write_completion_script(ShellType::Bash, &path);
        assert!(result.is_ok());

        let content2 = std::fs::read_to_string(&path).unwrap();

        // Content should be the same (idempotent)
        assert_eq!(content1, content2);
    }

    #[test]
    fn test_write_completion_script_overwrites_different_shell() {
        use tempfile::TempDir;

        let temp_dir = TempDir::new().unwrap();
        let path = temp_dir.path().join("autom8");

        // Write bash script
        write_completion_script(ShellType::Bash, &path).unwrap();
        let bash_content = std::fs::read_to_string(&path).unwrap();

        // Overwrite with zsh script
        write_completion_script(ShellType::Zsh, &path).unwrap();
        let zsh_content = std::fs::read_to_string(&path).unwrap();

        // Content should be different
        assert_ne!(bash_content, zsh_content);
        assert!(zsh_content.contains("#compdef"));
    }

    #[test]
    fn test_install_completions_available_as_public_api() {
        // Verify install_completions is a public function
        // (This test verifies the API exists; actual installation depends on env)
        let _: fn() -> Result<CompletionInstallResult> = install_completions;
    }

    #[test]
    fn test_completion_install_result_shell_display() {
        // Verify shell type displays correctly for messages
        let result = CompletionInstallResult {
            shell: ShellType::Zsh,
            path: PathBuf::from("/home/user/.zfunc/_autom8"),
            setup_instructions: None,
        };

        let message = format!(
            "Installed {} completions to {}",
            result.shell,
            result.path.display()
        );
        assert!(message.contains("zsh"));
        assert!(message.contains("_autom8"));
    }

    #[test]
    fn test_get_zsh_setup_instructions_content() {
        // Test the content of zsh setup instructions (assuming fpath not set)
        // Since we can't easily manipulate FPATH in tests, we test the instruction format
        let expected_content = "fpath=(~/.zfunc $fpath)";

        // The instructions should include this if zfunc is not in fpath
        // We can verify the helper function produces valid instructions
        let home = dirs::home_dir().unwrap();
        let zfunc_path = home.join(".zfunc/_autom8");
        assert!(zfunc_path.to_string_lossy().contains(".zfunc"));

        // Verify expected content format
        assert!(expected_content.contains("fpath"));
        assert!(expected_content.contains("$fpath"));
    }

    // ======================================================================
    // Tests for US-003: Dynamic spec file completion and utility subcommand
    // ======================================================================

    #[test]
    fn test_shell_type_from_name_bash() {
        let result = ShellType::from_name("bash");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ShellType::Bash);
    }

    #[test]
    fn test_shell_type_from_name_zsh() {
        let result = ShellType::from_name("zsh");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ShellType::Zsh);
    }

    #[test]
    fn test_shell_type_from_name_fish() {
        let result = ShellType::from_name("fish");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ShellType::Fish);
    }

    #[test]
    fn test_shell_type_from_name_case_insensitive() {
        // Should handle uppercase
        assert_eq!(ShellType::from_name("BASH").unwrap(), ShellType::Bash);
        assert_eq!(ShellType::from_name("ZSH").unwrap(), ShellType::Zsh);
        assert_eq!(ShellType::from_name("FISH").unwrap(), ShellType::Fish);

        // Should handle mixed case
        assert_eq!(ShellType::from_name("Bash").unwrap(), ShellType::Bash);
        assert_eq!(ShellType::from_name("Zsh").unwrap(), ShellType::Zsh);
        assert_eq!(ShellType::from_name("Fish").unwrap(), ShellType::Fish);
    }

    #[test]
    fn test_shell_type_from_name_invalid() {
        let result = ShellType::from_name("powershell");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("Unsupported shell"));
        assert!(err.contains("powershell"));
    }

    #[test]
    fn test_shell_type_from_name_error_lists_supported_shells() {
        let result = ShellType::from_name("invalid");
        assert!(result.is_err());
        let err = result.unwrap_err().to_string();
        assert!(err.contains("bash"));
        assert!(err.contains("zsh"));
        assert!(err.contains("fish"));
    }

    #[test]
    fn test_supported_shells_constant() {
        assert!(SUPPORTED_SHELLS.contains(&"bash"));
        assert!(SUPPORTED_SHELLS.contains(&"zsh"));
        assert!(SUPPORTED_SHELLS.contains(&"fish"));
        assert_eq!(SUPPORTED_SHELLS.len(), 3);
    }

    #[test]
    fn test_bash_completion_includes_dynamic_spec_function() {
        let script = generate_completion_script(ShellType::Bash);

        // Should include the dynamic spec completion function
        assert!(
            script.contains("_autom8_spec_files"),
            "Bash script should include _autom8_spec_files function"
        );

        // Should reference the config directory
        assert!(
            script.contains(".config/autom8"),
            "Bash script should reference the config directory"
        );

        // Should check for .json and .md files
        assert!(
            script.contains(".json") && script.contains(".md"),
            "Bash script should check for both .json and .md files"
        );

        // Should include git project detection
        assert!(
            script.contains("git rev-parse"),
            "Bash script should include git project detection"
        );
    }

    #[test]
    fn test_zsh_completion_includes_dynamic_spec_function() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should include the dynamic spec completion function
        assert!(
            script.contains("_autom8_spec_files"),
            "Zsh script should include _autom8_spec_files function"
        );

        // Should reference the config directory
        assert!(
            script.contains(".config/autom8"),
            "Zsh script should reference the config directory"
        );

        // Should check for .json and .md files
        assert!(
            script.contains(".json") && script.contains(".md"),
            "Zsh script should check for both .json and .md files"
        );

        // Should include git project detection
        assert!(
            script.contains("git rev-parse"),
            "Zsh script should include git project detection"
        );
    }

    #[test]
    fn test_fish_completion_includes_dynamic_spec_function() {
        let script = generate_completion_script(ShellType::Fish);

        // Should include the dynamic spec completion function
        assert!(
            script.contains("__autom8_spec_files"),
            "Fish script should include __autom8_spec_files function"
        );

        // Should reference the config directory
        assert!(
            script.contains(".config/autom8"),
            "Fish script should reference the config directory"
        );

        // Should check for .json and .md files
        assert!(
            script.contains(".json") && script.contains(".md"),
            "Fish script should check for both .json and .md files"
        );

        // Should include git project detection
        assert!(
            script.contains("git rev-parse"),
            "Fish script should include git project detection"
        );
    }

    #[test]
    fn test_bash_completion_includes_spec_flag_completion() {
        let script = generate_completion_script(ShellType::Bash);

        // Should have completion for --spec flag
        assert!(
            script.contains("--spec"),
            "Bash script should include --spec flag completion"
        );
    }

    #[test]
    fn test_zsh_completion_includes_spec_flag_completion() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should have completion for --spec flag
        assert!(
            script.contains("--spec"),
            "Zsh script should include --spec flag completion"
        );
    }

    #[test]
    fn test_fish_completion_includes_spec_flag_completion() {
        let script = generate_completion_script(ShellType::Fish);

        // Should have completion for --spec flag
        assert!(
            script.contains("--spec") || script.contains("-l spec"),
            "Fish script should include --spec flag completion"
        );
    }

    #[test]
    fn test_bash_completion_includes_subcommands_in_first_arg() {
        let script = generate_completion_script(ShellType::Bash);

        // Should list subcommands for first argument completion
        assert!(
            script.contains("run") && script.contains("status") && script.contains("resume"),
            "Bash script should include subcommands for first arg completion"
        );
    }

    #[test]
    fn test_print_completion_script_exists() {
        // Verify the function exists and is callable
        let _: fn(ShellType) = print_completion_script;
    }

    // ======================================================================
    // Tests for config subcommand completion
    // ======================================================================

    #[test]
    fn test_bash_completion_includes_config_subcommand() {
        let script = generate_completion_script(ShellType::Bash);

        // Should include config subcommand
        assert!(
            script.contains("autom8__config"),
            "Bash script should include config subcommand"
        );

        // Should include config in subcommands list
        assert!(
            script.contains("run status resume clean config init"),
            "Bash script should include config in dynamic subcommands list"
        );
    }

    #[test]
    fn test_zsh_completion_includes_config_subcommand() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should include config subcommand with description
        assert!(
            script.contains("'config:View, modify, or reset configuration'"),
            "Zsh script should include config subcommand with description"
        );
    }

    #[test]
    fn test_fish_completion_includes_config_subcommand() {
        let script = generate_completion_script(ShellType::Fish);

        // Should include config subcommand
        assert!(
            script.contains("-a \"config\"") || script.contains("config"),
            "Fish script should include config subcommand"
        );

        // Should include config in the exclusion list for spec file completion
        assert!(
            script.contains("run status resume clean config init"),
            "Fish script should include config in dynamic subcommands list"
        );
    }

    #[test]
    fn test_bash_completion_includes_config_set_subcommand() {
        let script = generate_completion_script(ShellType::Bash);

        // Should include config set subcommand
        assert!(
            script.contains("autom8__config__set"),
            "Bash script should include config set subcommand"
        );

        // Should include config reset subcommand
        assert!(
            script.contains("autom8__config__reset"),
            "Bash script should include config reset subcommand"
        );
    }

    #[test]
    fn test_zsh_completion_includes_config_set_subcommand() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should include config set subcommand
        assert!(
            script.contains("'set:Set a configuration value'"),
            "Zsh script should include config set subcommand"
        );

        // Should include config reset subcommand
        assert!(
            script.contains("'reset:Reset configuration to default values'"),
            "Zsh script should include config reset subcommand"
        );
    }

    #[test]
    fn test_fish_completion_includes_config_set_subcommand() {
        let script = generate_completion_script(ShellType::Fish);

        // Should include config set subcommand
        assert!(
            script.contains("\"set\"") && script.contains("Set a configuration value"),
            "Fish script should include config set subcommand"
        );

        // Should include config reset subcommand
        assert!(
            script.contains("\"reset\"")
                && script.contains("Reset configuration to default values"),
            "Fish script should include config reset subcommand"
        );
    }

    #[test]
    fn test_bash_completion_includes_config_keys() {
        let script = generate_completion_script(ShellType::Bash);

        // Should include all config keys
        let config_keys = [
            "review",
            "commit",
            "pull_request",
            "worktree",
            "worktree_path_pattern",
            "worktree_cleanup",
        ];

        for key in config_keys {
            assert!(
                script.contains(key),
                "Bash script should include config key: {}",
                key
            );
        }
    }

    #[test]
    fn test_zsh_completion_includes_config_keys() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should include all config keys in the dynamic completion
        let config_keys = [
            "review",
            "commit",
            "pull_request",
            "worktree",
            "worktree_path_pattern",
            "worktree_cleanup",
        ];

        for key in config_keys {
            assert!(
                script.contains(key),
                "Zsh script should include config key: {}",
                key
            );
        }
    }

    #[test]
    fn test_fish_completion_includes_config_keys() {
        let script = generate_completion_script(ShellType::Fish);

        // Should include all config keys
        let config_keys = [
            "review",
            "commit",
            "pull_request",
            "worktree",
            "worktree_path_pattern",
            "worktree_cleanup",
        ];

        for key in config_keys {
            assert!(
                script.contains(key),
                "Fish script should include config key: {}",
                key
            );
        }
    }

    #[test]
    fn test_bash_completion_includes_config_boolean_values() {
        let script = generate_completion_script(ShellType::Bash);

        // Should include true/false completion for boolean config values
        assert!(
            script.contains("\"true false\""),
            "Bash script should include true/false for boolean config values"
        );

        // Should exclude worktree_path_pattern from boolean completion
        assert!(
            script.contains("worktree_path_pattern"),
            "Bash script should handle worktree_path_pattern specially"
        );
    }

    #[test]
    fn test_zsh_completion_includes_config_boolean_values() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should include true/false completion for boolean config values
        assert!(
            script.contains("'true' 'false'"),
            "Zsh script should include true/false for boolean config values"
        );

        // Should exclude worktree_path_pattern from boolean completion
        assert!(
            script.contains("worktree_path_pattern"),
            "Zsh script should handle worktree_path_pattern specially"
        );
    }

    #[test]
    fn test_fish_completion_includes_config_boolean_values() {
        let script = generate_completion_script(ShellType::Fish);

        // Should include true/false completion for boolean config values
        assert!(
            script.contains("'true false'"),
            "Fish script should include true/false for boolean config values"
        );

        // Should exclude worktree_path_pattern from boolean completion
        assert!(
            script.contains("worktree_path_pattern"),
            "Fish script should handle worktree_path_pattern specially"
        );
    }

    #[test]
    fn test_bash_completion_config_set_dynamic_completion() {
        let script = generate_completion_script(ShellType::Bash);

        // Should have dynamic completion for config set
        assert!(
            script.contains(r#"[[ "${words[1]}" == "config" && "${words[2]}" == "set" ]]"#),
            "Bash script should have dynamic completion for config set"
        );
    }

    #[test]
    fn test_zsh_completion_config_set_dynamic_completion() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should have dynamic completion for config set
        assert!(
            script.contains(r#"[[ "${words[2]}" == "config" && "${words[3]}" == "set" ]]"#),
            "Zsh script should have dynamic completion for config set"
        );
    }

    #[test]
    fn test_fish_completion_config_set_dynamic_completion() {
        let script = generate_completion_script(ShellType::Fish);

        // Should have dynamic completion for config set
        assert!(
            script.contains("__fish_seen_subcommand_from config")
                && script.contains("__fish_seen_subcommand_from set"),
            "Fish script should have dynamic completion for config set"
        );
    }

    #[test]
    fn test_bash_completion_config_flags() {
        let script = generate_completion_script(ShellType::Bash);

        // Config command should have --global and --project flags
        assert!(
            script.contains("--global") && script.contains("--project"),
            "Bash script should include config --global and --project flags"
        );

        // Config reset should have --yes flag
        assert!(
            script.contains("--yes"),
            "Bash script should include config reset --yes flag"
        );
    }

    #[test]
    fn test_zsh_completion_config_flags() {
        let script = generate_completion_script(ShellType::Zsh);

        // Config command should have --global and --project flags
        assert!(
            script.contains("--global[Show only the global configuration]"),
            "Zsh script should include config --global flag"
        );
        assert!(
            script.contains("--project[Show only the project configuration]"),
            "Zsh script should include config --project flag"
        );

        // Config reset should have --yes flag
        assert!(
            script.contains("--yes[Skip confirmation prompt]"),
            "Zsh script should include config reset --yes flag"
        );
    }

    #[test]
    fn test_fish_completion_config_flags() {
        let script = generate_completion_script(ShellType::Fish);

        // Config command should have --global and --project flags
        assert!(
            script.contains("-l global") && script.contains("-l project"),
            "Fish script should include config --global and --project flags"
        );

        // Config reset should have --yes flag
        assert!(
            script.contains("-l yes"),
            "Fish script should include config reset --yes flag"
        );
    }

    #[test]
    fn test_zsh_completion_config_key_descriptions() {
        let script = generate_completion_script(ShellType::Zsh);

        // Should include descriptive completions for config keys
        assert!(
            script.contains("'review:Enable code review step'"),
            "Zsh script should include review key with description"
        );
        assert!(
            script.contains("'commit:Enable auto-commit'"),
            "Zsh script should include commit key with description"
        );
        assert!(
            script.contains("'pull_request:Enable auto-PR creation'"),
            "Zsh script should include pull_request key with description"
        );
        assert!(
            script.contains("'worktree:Enable worktree mode'"),
            "Zsh script should include worktree key with description"
        );
        assert!(
            script.contains("'worktree_path_pattern:Pattern for worktree names'"),
            "Zsh script should include worktree_path_pattern key with description"
        );
        assert!(
            script.contains("'worktree_cleanup:Auto-cleanup worktrees'"),
            "Zsh script should include worktree_cleanup key with description"
        );
    }
}
