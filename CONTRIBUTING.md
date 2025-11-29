# Contributing to ofsht

Thank you for your interest in contributing to ofsht! This document provides guidelines and information for contributors.

## Table of Contents

- [Development Setup](#development-setup)
- [Development Workflow](#development-workflow)
- [Code Architecture](#code-architecture)
- [Testing](#testing)
- [Coding Guidelines](#coding-guidelines)
- [Documentation Style](#documentation-style)
- [Submitting Changes](#submitting-changes)

## Development Setup

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) 1.70+ (2021 edition)
- [mise](https://mise.jdx.dev/) - Development tool version manager (includes just and vhs)
- Git 2.5+ (for worktree support)

#### Installing mise

**macOS/Linux (Homebrew)**:
```bash
brew install mise
```

**Linux/macOS (Script)**:
```bash
curl https://mise.run | sh
```

**Other platforms**: See [mise installation guide](https://mise.jdx.dev/getting-started.html)

> [!TIP]
> **Dogfooding**: ofsht itself is also installable via mise: `mise use -g ubi:wadackel/ofsht`
>
> Note: `mise install` (without arguments) only installs development tools from `mise.toml` (just, vhs). Installing ofsht via mise is optional and requires the explicit `mise use -g` command above.

#### Installing Development Tools

After installing mise:
```bash
mise install  # Installs just and vhs as specified in mise.toml
```

This will install:
- `just` 1.43.1 - Command runner for development tasks
- `vhs` (latest) - Terminal demo video generator (optional, for documentation work)

### Building from Source

```bash
# Clone the repository
git clone https://github.com/wadackel/ofsht.git
cd ofsht

# Install development tools
mise install

# Build the project
cargo build

# Build with optimizations
cargo build --release

# Run all quality checks
just check
```

### Testing the CLI During Development

```bash
# Run the CLI in development mode
cargo run -- add feature-branch
cargo run -- ls
cargo run -- cd feature-branch
cargo run -- rm .
cargo run -- rm /path/to/worktree
cargo run -- rm feature-a feature-b  # Remove multiple worktrees
cargo run -- rm feature-a . feature-b  # Remove multiple including current

# Use release binary
./target/release/ofsht add feature-branch
```

See [TEST.md](./TEST.md) for comprehensive manual testing procedures.

## Development Workflow

This project follows **t_wada's philosophy**:
- **TDD**: Write tests first, then minimal implementation
- **Small steps**: Incremental changes with test verification
- **YAGNI**: Only implement what's needed now
- **Quality built-in**: All checks must pass before committing (`just check`)

### Available Just Recipes

**Development** (fast feedback):
- `just build` - Build the project
- `just test` - Run tests (simple)
- `just fmt` - Check code formatting (simple)
- `just clippy` - Run clippy linter (simple)

**CI-Equivalent** (strict):
- `just check` - Run all checks (fmt-ci + clippy-ci + test-ci)
- `just fmt-ci` - Check formatting (all packages)
- `just clippy-ci` - Run clippy (all targets, locked dependencies)
- `just test-ci` - Run tests (all targets, no-fail-fast)

**Utilities**:
- `just install` - Install ofsht to cargo bin directory
- `just clean` - Clean build artifacts
- `just help` (or `just --list`) - Show all available recipes

### Running Tests

```bash
# Run all tests
cargo test

# Run all tests (CI-equivalent)
just test-ci

# Run specific test module
cargo test config::tests
cargo test integrations::zoxide::tests
```

See [TEST_COVERAGE.md](./TEST_COVERAGE.md) for comprehensive test coverage tracking and metrics.

### Code Quality Checks

```bash
# Check formatting
just fmt-ci

# Run clippy with strict warnings
just clippy-ci

# Run all checks (required before committing)
just check
```

## Code Architecture

### Refactoring Benefits (v0.2.0)

The codebase underwent comprehensive refactoring in [PR #43](https://github.com/wadackel/ofsht/pull/43) to improve:
- **Maintainability**: `main.rs` reduced from ~1800 to ~160 lines (90% reduction)
- **Testability**: 230 tests total (177 bin + 53 integration) with clear module boundaries
- **Code Organization**: Commands, domain logic, and integrations properly separated into focused modules
- **Error Handling**: All `unwrap()` calls replaced with explicit error handling using `let-else` and pattern matching

### Module Structure

The codebase follows a modular design with clear separation of concerns:

```
src/
├── main.rs              # CLI entry point and command routing
├── cli.rs               # CLI argument definitions (clap)
├── color.rs             # Color output utilities
├── hooks.rs             # Hook execution engine (run/copy/link)
├── service.rs           # Service layer orchestrating git, hooks, and integrations
├── commands/
│   └── common.rs        # Shared utilities for command handlers
├── config/
│   ├── mod.rs           # Config module root (re-exports)
│   ├── schema.rs        # Type definitions and templates
│   └── loader.rs        # Configuration loading logic
├── domain/
│   └── worktree.rs      # Domain entities and parsers
└── integrations/
    ├── mod.rs           # Integrations module root
    ├── fzf.rs           # fzf integration for interactive selection
    ├── git.rs           # Git client abstraction
    ├── tmux.rs          # tmux integration for window/pane creation
    ├── zoxide.rs        # zoxide integration with graceful degradation
    └── gh/              # GitHub CLI integration
        ├── mod.rs
        ├── client.rs
        └── input.rs
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
  - **Integration settings (`zoxide`, `fzf`, `tmux`, `gh`) are ONLY available in global config**
- Default config: Hard-coded defaults if no files exist
- The `Config::load_impl()` function handles this cascading logic

**Hook Execution Flow** (`hooks.rs`)
1. Commands run first (`actions.run`)
2. Files copied second (`actions.copy`)
3. Symlinks created last (`actions.link`)
- All hooks execute in the worktree directory context
- Source files are resolved from the original repository
- Missing source files emit warnings but don't fail execution
- **Output Streams**: Hook command stdout/stderr are redirected to stderr to prevent polluting the stdout stream

**Graceful Degradation for zoxide** (`integrations/zoxide.rs`)
- Checks `zoxide --version` to detect availability
- If zoxide is not installed, silently skips registration
- If installed but `enabled = false` in config, skips registration
- Only fails if zoxide is installed, enabled, and returns an error

**Interactive Selection with fzf** (`integrations/fzf.rs`)
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

[integration.gh]
enabled = true  # Default: true (only available in global config)
```

## Testing

### Testing Strategy

- Each module has a `#[cfg(test)]` section with unit tests
- Test-first development: write test, see it fail, implement, see it pass
- Mock external dependencies (e.g., `MockZoxideClient` in `zoxide.rs`)
- Integration tests verify actual git and zoxide commands work
- Shell completion tests (`tests/completions.rs`): smoke tests that verify generation succeeds and basic commands are present

### Manual Testing

See [TEST.md](./TEST.md) for comprehensive manual testing procedures covering:
- Worktree creation and navigation
- Hook execution
- zoxide integration
- fzf interactive selection
- Shell completion
- Configuration loading
- Error handling

### Demo Videos with VHS

Demo videos are generated using VHS from `.tape` files in `docs/assets/vhs/`.

**Generate a specific demo**:
```bash
just demo quick-start
```

**Generate all demos**:
```bash
just demo-all
```

**Validate tape files** (dry-run, no GIF generation):
```bash
just demo-verify
```

> [!NOTE]
> When contributing documentation changes, run `just demo-verify` to ensure tape files remain valid. Generated GIFs are committed to the repository for use in README.md.

## Coding Guidelines

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

### Platform-Specific Code

Symlink creation differs between Unix and Windows (`hooks.rs:92-115`):
- Unix: `std::os::unix::fs::symlink`
- Windows: `symlink_dir` vs `symlink_file` based on source type

## Documentation Style

When contributing to documentation (README.md, CONTRIBUTING.md, TEST.md), follow these GitHub Alert patterns:

### GitHub Alerts Usage

Use GitHub-style alerts to highlight important information. Choose the appropriate alert type based on the content:

#### Alert Types and When to Use

**`> [!NOTE]`** - Factual clarifications or reminders (no action required)
- Capabilities, supported platforms, background context
- Reassuring users about edge cases
- Example: "ofsht can remove worktrees even if directories were manually deleted"

**`> [!TIP]`** - Productivity hints and optional shortcuts
- Alternate commands or workflows
- Recommended sequences that improve user experience
- Graceful degradation explanations
- Example: "Use `create` instead of `add` when you don't want auto-navigation"

**`> [!IMPORTANT]`** - Prerequisites and silent failure conditions
- Hard requirements before running a command
- Configuration constraints that silently disable features
- Settings that must be in specific files
- Example: "Integration settings are ONLY read from global config"

**`> [!WARNING]`** - Actions that destroy state (normal but destructive)
- Commands that overwrite files or delete data
- Permanent changes that can't be undone
- Example: "`ofsht init --force` will overwrite existing config files"

**`> [!CAUTION]`** - Environment-sensitive requirements
- Commands that require specific runtime environments
- Authentication or session prerequisites
- Platform-specific limitations
- Example: "tmux integration requires running inside an active tmux session"

### Alert Best Practices

- **Be concise**: Keep alerts to 2-3 sentences maximum
- **Be specific**: Mention exact commands, files, or error messages when relevant
- **Avoid alert proliferation**: Only use alerts for truly important callouts
- **Consistent formatting**: Use the same visual language throughout all docs
- **Test clarity**: If you can't decide between NOTE and TIP, it might not need an alert

### Examples from the Codebase

Good alert usage in README.md:
```markdown
> [!TIP]
> The `add` command automatically navigates to the new worktree when shell integration is enabled. Use `create` instead if you want to stay in the current directory.

> [!IMPORTANT]
> Integration settings (`[integration.zoxide]`, `[integration.fzf]`, etc.) are ONLY read from global config. Placing them in `.ofsht.toml` will silently have no effect.

> [!CAUTION]
> tmux integration requires running `ofsht add` inside an active tmux session. Using `--tmux` outside of tmux will fail with an error.
```

## Submitting Changes

### Pull Request Process

1. Fork the repository and create a feature branch
2. Make your changes following the TDD workflow
3. Ensure all tests pass: `just check`
4. Write clear, descriptive commit messages
5. Push your branch and create a pull request
6. Address any review feedback

### Commit Message Guidelines

This project follows the [Conventional Commits](https://www.conventionalcommits.org/) specification. This enables automatic changelog generation and semantic versioning for releases.

#### Format

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

#### Commit Types

| Type | Description | Changelog Section | Version Bump |
|------|-------------|-------------------|--------------|
| `feat` | New feature | Added | Minor (0.x.0) |
| `fix` | Bug fix | Fixed | Patch (0.0.x) |
| `perf` | Performance improvement | Performance | Patch |
| `refactor` | Code refactoring | Changed | Patch |
| `docs` | Documentation changes | Documentation | Patch |
| `test` | Test additions/changes | Testing | Patch |
| `build` | Build system changes | Build | Patch |
| `deps` | Dependency updates | Dependencies | Patch |
| `ci` | CI/CD changes | CI/CD | Patch |
| `chore` | Other maintenance | Miscellaneous | Patch |
| `style` | Code style changes | Styling | Patch |

#### Breaking Changes

Breaking changes trigger a major version bump (x.0.0 in 1.x, or 0.x.0 in 0.x):

```bash
# Method 1: Use BREAKING CHANGE in footer
git commit -m "feat: redesign config file format

BREAKING CHANGE: Config file format changed from JSON to TOML"

# Method 2: Add ! after type
git commit -m "feat!: redesign config file format"
```

#### Examples

```bash
# Feature addition (bumps minor version)
git commit -m "feat: add fzf integration for interactive worktree selection"

# Bug fix (bumps patch version)
git commit -m "fix: resolve terminal hang by properly handling stdio pipes"

# Performance improvement
git commit -m "perf: optimize worktree path resolution"

# Documentation update
git commit -m "docs: standardize all content to English"

# Dependency update
git commit -m "deps: update clap to 4.5.0"

# Breaking change
git commit -m "feat!: change default worktree directory template"
```

#### Optional Scope

Add scope to provide additional context:

```bash
git commit -m "feat(tmux): add pane creation support"
git commit -m "fix(config): resolve XDG_CONFIG_HOME path correctly"
git commit -m "docs(hooks): document run/copy/link actions"
```

> [!NOTE]
> Conventional Commits format is **required** for proper changelog generation. All pull requests must follow this convention.

## Release Process

### Automated Release Flow

ofsht uses an automated release process powered by [release-plz](https://release-plz.dev/):

1. **Development**: Make changes following conventional commits
2. **Release PR**: release-plz automatically creates/updates a PR with:
   - Version bump based on commit types
   - Updated CHANGELOG.md
   - Updated Cargo.toml version
3. **Merge**: Merge the release PR to trigger the release
4. **Automated Publishing**:
   - GitHub release is created
   - Binaries are built for all platforms (macOS arm64/x86_64, Linux x86_64/musl)
   - Binaries are attached to the GitHub release
   - Package is published to crates.io
   - **Homebrew formula is automatically updated via PR**

### Homebrew Distribution

The Homebrew tap (`wadackel/homebrew-tap`) is automatically updated on each release:

1. **Trigger**: The `update-homebrew-tap` job runs after binaries are uploaded
2. **Workflow**: Triggers `bump-formula.yaml` in the tap repository
3. **Formula Update**: Downloads release binaries, calculates SHA256, updates formula
4. **Pull Request**: Creates PR in tap repository with updated formula
5. **Review & Merge**: Manually review and merge the PR to publish the update

**Tap Repository**: https://github.com/wadackel/homebrew-tap

**Troubleshooting Homebrew Updates**:

- If the tap update fails, check the [tap repository actions](https://github.com/wadackel/homebrew-tap/actions)
- The main release will still succeed even if tap update fails
- Tap updates can be triggered manually via the tap repository's Actions tab
- Required secrets/variables:
  - `OFSHT_APP_ID` (variable): GitHub App ID for tap repository access
  - `OFSHT_APP_PRIVATE_KEY` (secret): GitHub App private key
  - GitHub App must be installed on both `ofsht` and `homebrew-tap` repositories
  - GitHub App permissions required: actions (read/write), contents (read)

### mise ubi Distribution

ofsht binaries are automatically compatible with mise's [ubi backend](https://mise.jdx.dev/dev-tools/backends/ubi.html):

```bash
mise use -g ubi:wadackel/ofsht
```

**How it works**:
1. mise's ubi backend reads GitHub releases directly from this repository
2. Automatically detects the user's platform (OS + CPU architecture)
3. Downloads and extracts the appropriate binary from release assets
4. No maintainer action or registry registration required

**Requirements** (already met):
- ✅ Asset naming follows platform conventions: `ofsht-${target}.tar.gz`
- ✅ Archive contains single executable at root level
- ✅ Supported platforms: Linux (x86_64 gnu/musl), macOS (x86_64/aarch64)

**Important**: Asset naming is critical for mise compatibility. The release workflow (`.github/workflows/release.yaml:181-185`) generates archives in the format `ofsht-{target}.tar.gz` containing a single `ofsht` binary. Changing this format will break mise installations.

**Registry decision**: We intentionally do NOT register ofsht in the [mise registry](https://mise.jdx.dev/registry.html). The ubi backend works perfectly without registration, and users can install via `ubi:wadackel/ofsht` directly. This avoids the maintenance burden of keeping a registry entry updated.

### Manual Release

If you need to trigger a release manually:

1. Ensure all changes are committed and pushed
2. Use the GitHub Actions UI to run the release workflow
3. Or create a release tag manually: `git tag v0.x.y && git push origin v0.x.y`

> [!WARNING]
> Manual releases bypass the conventional commit validation and may result in incorrect version bumps or changelogs.

### Code Review

- All submissions require review before merging
- Address review comments promptly
- Be open to feedback and suggestions
- Ensure CI checks pass before requesting review

## Questions or Issues?

- Open an issue for bugs or feature requests
- Start a discussion for questions or design proposals
- Check existing issues before creating new ones

Thank you for contributing to ofsht!
