# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

> **Note**: For general development information, setup instructions, and contribution guidelines, see [CONTRIBUTING.md](../CONTRIBUTING.md). This file contains Claude Code-specific guidance and detailed architecture information.

## Project Overview

**ofsht** is a Git worktree management CLI tool written in Rust. It wraps `git worktree` commands and adds automation features like hooks, file copying, symlink creation, and zoxide integration.

## Development Commands

### Essential Commands
```bash
# Run all quality checks before committing (required)
just check

# Build the project
cargo build
# or for release
cargo build --release

# Run all tests
cargo test
# or CI-equivalent
just test-ci

# Run specific test module
cargo test config::tests
cargo test zoxide::tests

# Check formatting
just fmt-ci

# Run clippy with strict warnings
just clippy-ci
```

### Testing the CLI
```bash
# Run the CLI in development
cargo run -- add feature-branch
cargo run -- ls
cargo run -- cd feature-branch
cargo run -- rm .
cargo run -- rm /path/to/worktree
cargo run -- rm feature-a feature-b  # Remove multiple worktrees
cargo run -- rm feature-a . feature-b  # Remove multiple including current

# Test tmux integration (must be run inside tmux)
cargo run -- add feature-branch --tmux

# Use release binary
./target/release/ofsht add feature-branch
```

See `docs/TEST.md` for comprehensive manual testing procedures.

## Code Architecture

### Module Structure

The codebase follows a modular design with clear separation of concerns:

```
src/
├── main.rs           # CLI entry point and command routing only (~160 lines)
├── cli.rs            # CLI definitions and completion logic
├── commands/         # Command handlers (extracted from main.rs)
│   ├── add.rs        # Add command with GitHub/tmux integration
│   ├── cd.rs         # Navigate to worktree
│   ├── common.rs     # Shared command utilities
│   ├── completion.rs # Generate shell completions
│   ├── create.rs     # Simple worktree creation
│   ├── init.rs       # Initialize config files
│   ├── list.rs       # List worktrees
│   ├── rm.rs         # Remove worktrees
│   └── shell_init.rs # Generate shell integration scripts
├── config.rs         # TOML configuration loading (local + global)
├── domain/           # Domain models and logic
│   └── worktree.rs   # Worktree entry parsing and formatting
├── hooks.rs          # Hook execution engine (run/copy/link)
├── integrations/     # External tool integrations
│   ├── fzf/          # Interactive selection
│   ├── gh/           # GitHub CLI integration
│   ├── git/          # Git operations abstraction
│   ├── tmux/         # Tmux window/pane creation
│   └── zoxide/       # Directory tracking
├── service/          # Business logic layer
│   └── worktree.rs   # Worktree creation service
└── color.rs          # Terminal color output
```

### Key Design Patterns

**Two-tier Configuration System** (`config/loader.rs`)
- Local config: `.ofsht.toml` in **main repository root** (highest priority)
  - **Always loaded from main repository root**, not from individual worktrees
  - Use `Config::load_from_repo_root(&repo_root)` for git operations
  - Ensures consistent behavior across all worktrees
  - **Integration settings are NOT read from local config** - they are always sourced from global config
- Global config: Respects `XDG_CONFIG_HOME` on all platforms
  - Uses `$XDG_CONFIG_HOME/ofsht/config.toml` if set (must be absolute path)
  - Fallbacks to `~/.config/ofsht/config.toml` otherwise
  - **Integration settings (`zoxide`, `fzf`, `tmux`) are ONLY available in global config**
- Default config: Hard-coded defaults if no files exist
- The `Config::load_impl()` function handles this cascading logic:
  1. If local config exists: Load worktree/hooks settings from local, integration settings from global (or defaults)
  2. If only global config exists: Load all settings from global
  3. If neither exists: Use default config

**Hook Execution Flow** (`hooks.rs`)
1. Commands run first (`actions.run`)
2. Files copied second (`actions.copy`)
3. Symlinks created last (`actions.link`)
- All hooks execute in the worktree directory context
- Source files are resolved from the original repository
- Missing source files emit warnings but don't fail execution
- **Output Streams**: Hook command stdout/stderr are redirected to stderr to prevent polluting the stdout stream (required for shell integration to work correctly)

**Graceful Degradation for zoxide** (`integrations/zoxide.rs`)
- Checks `zoxide --version` to detect availability
- If zoxide is not installed, silently skips registration
- If installed but `enabled = false` in config, skips registration
- Only fails if zoxide is installed, enabled, and returns an error

**tmux Integration** (`integrations/tmux.rs`)
- Configurable via `behavior` setting: "auto" (default, flag-based), "always" (always enabled), "never" (disabled)
- CLI flags override config: `--tmux` (enable), `--no-tmux` (disable)
- Priority: `--no-tmux` > `--tmux` > `behavior` setting
- Early detection before git worktree add (fail fast if not in tmux session)
- Window/pane creation happens AFTER git/hooks/zoxide (ensures worktree is ready)
- `TmuxLauncher::detect()`: Checks $TMUX env var and tmux binary availability
- `TmuxLauncher::create_window()`: Creates tmux window with sanitized branch name
- `TmuxLauncher::create_pane()`: Creates horizontal split pane in current window
- `sanitize_window_name()`: Replaces `/` and spaces with `·`, truncates to 50 chars
- `create` config determines what to create: "window" (default) or "pane"
- Failures are warnings only (worktree preserved, error goes to stderr)

**Interactive Selection with fzf** (`integrations/fzf/`)
- When `ofsht cd` or `ofsht rm` is run without arguments, fzf launches for interactive selection
- Graceful degradation: returns error if fzf is disabled or not installed
- Uses `FzfPicker` trait for testability (`MockFzfPicker` in tests)
- `build_worktree_items()` parses `git worktree list --porcelain` output
- `RealFzfPicker::pick()` handles multi-select mode (for `rm`) and single-select (for `cd`)
- Exit codes 130/1 (Esc/no selection) are treated as Ok(vec![]) rather than errors
- Preview window shows `git log --oneline -n 10` for each worktree

**Path Template Expansion** (`commands/add.rs`, `commands/create.rs`)
- Template variables: `{repo}` and `{branch}`
- Supports both absolute paths (`/tmp/worktrees/...`) and relative paths (`../{repo}-worktrees/...`)
- Repository name extracted from main repository root directory name
- Relative paths are resolved from main repository root (not current directory)
- Default template: `../{repo}-worktrees/{branch}`
- Uses `git rev-parse --git-common-dir` to find main repository (requires Git ≥2.5, 2015)

### Command Implementation

All commands in `src/commands/` modules follow a consistent pattern:
1. Get main repository root via `get_main_repo_root()` (from `commands/common.rs`)
2. Load configuration via `Config::load_from_repo_root()`
3. Execute git command via `Command::new("git")`
4. Check exit status and handle stderr
5. Execute side effects (hooks, zoxide, tmux)

**Command Modules** (in `src/commands/`):
- **add.rs** (`cmd_new`): GitHub integration (PR/issue detection) → worktree creation → hooks → zoxide → optional tmux window/pane
- **cd.rs** (`cmd_goto`): Parse worktree list → find by branch name → optional fzf selection → print path for shell integration
- **common.rs**: Shared utilities (`get_main_repo_root`, `parse_all_worktrees`, `resolve_worktree_target`, etc.)
- **completion.rs** (`cmd_completion`): Print shell-specific completion setup instructions
- **create.rs** (`cmd_create`): Simple worktree creation without GitHub/tmux (deprecated command)
- **init.rs** (`cmd_init`): Generate global/local config templates
- **list.rs** (`cmd_list`): Format and display worktree list (interactive vs pipe mode)
- **rm.rs** (`cmd_rm_many`): Multi-target removal with fzf support → duplicate detection → current worktree last
- **shell_init.rs** (`cmd_shell_init`): Generate shell wrapper functions for cd/add/rm integration

**Key Shared Functions** (in `commands/common.rs`):
- `get_main_repo_root()`: Find main repo using `git rev-parse --git-common-dir`
- `resolve_worktree_target()`: Resolve branch name, path, or `.` to canonical worktree path
- `parse_all_worktrees()`: Parse `git worktree list --porcelain` output
- `find_worktree_by_branch()`: Locate worktree path by branch name

### Configuration Schema

```toml
[worktree]
dir = "../{repo}-worktrees/{branch}"

[hooks.create]
run = ["command1", "command2"]  # Shell commands
copy = ["file1", "file2"]       # Files to copy
link = { "src" = "dest" }       # Symlinks (src in repo -> dest in worktree)

[hooks.delete]
run = ["command1"]
# Note: copy and link actions are not supported in delete hooks

[integration.zoxide]
enabled = true  # Default: true (only available in global config)

[integration.fzf]
enabled = true  # Default: true (only available in global config)
options = ["--height=50%", "--border"]  # Additional fzf command-line options

[integration.tmux]
behavior = "auto"   # "auto" (flag-based, default), "always", "never"
create = "window"   # "window" (default), "pane"
```

## Development Philosophy

This project follows **t_wada's philosophy**:
- **TDD**: Write tests first, then minimal implementation
- **Small steps**: Incremental changes with test verification
- **YAGNI**: Only implement what's needed now
- **Quality built-in**: All checks must pass before committing (`just check`)

### Testing Strategy

- Each module has a `#[cfg(test)]` section with unit tests
- Test-first development: write test, see it fail, implement, see it pass
- Mock external dependencies (e.g., `MockZoxideClient` in `integrations/zoxide.rs`)
- Integration tests verify actual git and zoxide commands work
- Shell completion tests (`tests/completions.rs`): smoke tests that verify generation succeeds and basic commands are present

### Clippy Configuration

Strict linting is enabled in `Cargo.toml`:
```toml
[lints.clippy]
all = { level = "warn", priority = -1 }
pedantic = { level = "warn", priority = -1 }
nursery = { level = "warn", priority = -1 }
```

Common allowed lints in code:
- `#[allow(clippy::literal_string_with_formatting_args)]` for path templates
- `#[allow(clippy::missing_const_for_fn)]` is fixed by making functions `const`

## Project Status

Current phase: **v0.2.0 Development**

Completed:
- ✅ Phase 0: CLI framework setup
- ✅ Phase 1: Basic operations (add, ls, cd, rm)
- ✅ Phase 2: Hook functionality
- ✅ Phase 3: `ofsht init` command to generate config templates
- ✅ Phase 4: zoxide integration
- ✅ Phase 5: Shell completion generation (Bash, Zsh, Fish)
- ✅ v0.2.0: Breaking changes (path template, repo root resolution, XDG config)

Remaining:
- 🚀 Phase 6: crates.io release

## Important Implementation Notes

### Platform-Specific Code

Symlink creation differs between Unix and Windows (`hooks.rs`):
- Unix: `std::os::unix::fs::symlink`
- Windows: `symlink_dir` vs `symlink_file` based on source type

### Error Handling

Uses `anyhow::Result` throughout with `.context()` for error context:
```rust
.with_context(|| format!("Failed to read config file: {}", path.display()))?
```

### Git Integration

All git operations use `std::process::Command` with:
- `.output()` to capture stdout/stderr
- Status code checking
- UTF8 conversion with `String::from_utf8_lossy`

### zoxide Integration Timing

zoxide registration happens **after** hooks execute to ensure the worktree is fully set up (dependencies installed, etc.) before it appears in zoxide's database.

### Shell Completion

Uses `clap_complete` crate for runtime generation via the `completions` subcommand:
- Supported shells: Bash, Zsh, Fish
- Shell type parsed from string argument using `Shell::from_str()`
- Generates to stdout via `generate(shell, &mut Cli::command(), "ofsht", &mut io::stdout())`
- Invalid shell names return user-friendly error with supported shell list

### Start Point for Worktree Creation

The `add` command accepts an optional `start_point` argument:
- Syntax: `ofsht add <branch> [start-point]`
- Start point can be: branch name, tag, commit hash, or relative commit (e.g., `HEAD~3`)
- If omitted, defaults to current HEAD
- Examples:
  - `ofsht add feature` → creates from HEAD
  - `ofsht add feature develop` → creates from develop branch
  - `ofsht add feature v1.0.0` → creates from tag v1.0.0
  - `ofsht add feature origin/main` → creates from remote branch
- Implementation: Passes start point directly to `git worktree add -b <branch> <path> [start-point]`
- Error handling: Git validates the start point and returns errors for invalid refs

### Shell Integration (wtp-style)

**Architecture**: The `shell-init` subcommand generates shell-specific wrapper functions that intercept `cd`, `add`, and `rm` subcommands for automatic directory changing.

**Technical Constraint**: Rust binaries cannot change the parent shell's current directory due to process isolation. The shell integration solves this by generating wrapper functions that:
1. Call the `ofsht` binary via `command ofsht` (to avoid infinite recursion)
2. Capture the output (path) to stdout
3. Execute `cd` in the shell process itself

**Implementation** (`commands/shell_init.rs`):
- `cmd_shell_init(shell: &str)` - Returns shell-specific wrapper script
- Templates stored in `templates/` directory
- Uses `include_str!()` macro to embed templates at compile time
- Supported shells: bash, zsh, fish

**Template Structure** (`templates/*.{sh,fish}`):
```bash
# All templates follow the same pattern:
ofsht() {  # or `function ofsht` in Fish
    if [[ "$1" == "cd" ]] || [[ "$1" == "add" ]] || [[ "$1" == "rm" ]]; then
        # Capture output and cd to it
        local result=$(command ofsht "$@") || return $?
        [[ -n "$result" ]] && cd -- "$result"
    else
        # Pass through all other subcommands
        command ofsht "$@"
    fi
}
```

**Key Design Decisions**:
- **wtp-style wrapping**: All `ofsht` commands go through the wrapper function (not separate `ofsht_cd` / `ofsht_add` functions like zoxide)
- **POSIX compatibility**: Uses `command` keyword to call the binary, avoiding function shadowing issues
- **`cd --` usage**: Protects against paths starting with `-`
- **Empty check**: Verifies output before attempting `cd` to handle errors gracefully
- **Error propagation**: Preserves exit codes with `|| return $?`

**User Experience**:
```bash
# Setup (one-time)
eval "$(ofsht shell-init bash)"

# After setup, seamless navigation
ofsht add feature    # Creates and navigates
ofsht cd feature     # Navigates to existing worktree
ofsht rm .           # Removes current worktree and returns to main
ofsht rm feature     # Removes specified worktree (no directory change)
ofsht ls             # Normal operation (no directory change)
```

**Shell-Specific Differences**:
- **Bash/Zsh**: Nearly identical, uses `[[ ]]` conditionals and `local` variables
- **Fish**: Uses `test` command, `set -l` for local variables, `function...end` syntax

**Testing**: Manual testing required for each shell (see docs/TEST.md). No automated tests due to shell environment dependencies.

## CI/CD Workflows

### Release Workflow (`.github/workflows/release.yaml`)

The release workflow uses **release-plz** to automate Rust crate releases with GitHub App authentication.

**Architecture**:
1. **release-plz-pr** job: Creates/updates release PR with changelog
2. **release-plz-release** job: Publishes release when PR is merged to main
3. **build-binaries** job: Builds cross-platform binaries and attaches to release

**Authentication**:
- Uses GitHub App token instead of PAT for better security and control
- Both `release-plz-pr` and `release-plz-release` jobs generate short-lived tokens via `actions/create-github-app-token`
- Required secrets/vars:
  - `vars.OFSHT_APP_ID`: GitHub App ID
  - `secrets.OFSHT_APP_PRIVATE_KEY`: GitHub App private key
  - `secrets.CARGO_REGISTRY_TOKEN`: crates.io API token
- GitHub App permissions required:
  - Repository permissions → Contents: Read and write
  - Repository permissions → Pull requests: Read and write
  - Repository permissions → Metadata: Read-only (automatic)

**Critical Configuration**:
- **Action version consistency**: Both PR and release jobs MUST use the same `release-plz/action` version (currently `v0.5.118`)
  - Older versions (e.g., `v0.5.101`) have different Git authentication behavior and may fail
  - The action internally handles Git credential configuration when provided with a GitHub App token
- **Checkout settings**: Use `persist-credentials: false` for security (the action manages its own authentication)
- **SHA pinning**: All external actions are SHA-pinned for supply chain security

**Common Issues**:
- Git authentication failures: Ensure both jobs use the same action version and GitHub App token is properly configured
- Permission errors: Verify GitHub App has required permissions and is installed on the repository
- Release draft not created: Check `release-plz.toml` has `git_release_enable = true` and `git_release_draft = true`

**Testing CI Changes**:
- Use `gh` CLI to inspect workflow runs: `gh run view <run-id>`
- Check job logs: `gh run view <run-id> --job=<job-id>`
- Verify GitHub App installation: Settings → GitHub Apps → Installed GitHub Apps

### Commit Message Convention

This project uses [Conventional Commits](https://www.conventionalcommits.org/):
- `feat:` - New features
- `fix:` - Bug fixes
- `docs:` - Documentation changes
- `refactor:` - Code refactoring
- `test:` - Test additions/modifications
- `chore:` - Maintenance tasks
- `ci:` - CI/CD workflow changes
- `perf:` - Performance improvements

## Language Policy

### Documentation & Code
- All documentation files (README.md, CONTRIBUTING.md, docs/TEST.md, etc.) must be in English
- All code comments and docstrings must be in English
- All error messages and user-facing text must be in English
- All commit messages must be in English
- Use clear, concise technical writing matching this document's style

### Claude Code Interactions
- Match the user's language in conversations
- If user writes in Japanese, respond in Japanese
- If user writes in English, respond in English
- Keep code/documentation references in English regardless of conversation language
