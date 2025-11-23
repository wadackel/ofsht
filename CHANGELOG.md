# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
## [0.1.8] - 2025-11-23

### Breaking Changes

- Add global git credentials for release-plz operations

- Align release-plz workflow with official best practices

- Configure global git credentials for release-plz

- Enable persist-credentials for release-plz git operations

- Set persist-credentials to false for security best practice

- Enable persist-credentials for release-plz git operations

- Configure git credentials for release-plz push

- Update release-plz/action to v0.5.101 for GitHub App support

- Use GitHub App token for release PR creation

- Migrate from PAT to GitHub App authentication ([#10](https://github.com/wadackel/ofsht/pull/10))


## [Unreleased]

## [0.1.7] - 2025-11-23

### Breaking Changes

- Update workflow filename reference to update.yaml


## [0.1.6] - 2025-11-23

### Breaking Changes

- Update workflow filename to update-formula.yaml


## [0.1.5] - 2025-11-23

### Breaking Changes

- Enable crates.io installation instructions

- Update Homebrew installation and release documentation

- Monitor Homebrew tap workflow completion

- Add Homebrew tap auto-update on release


## [0.1.4] - 2025-11-23

### Breaking Changes

- Add --repo flag to gh release commands


## [0.1.3] - 2025-11-23

### Breaking Changes

- Use correct output name for release-plz releases


## [0.1.2] - 2025-11-23

### Breaking Changes

- Make badges clickable with relevant links

- Add continue-on-error to cache steps for macOS compatibility


## [0.1.1] - 2025-11-23

### Breaking Changes

- Extract tag from release-plz output instead of git describe


### Changed
- **BREAKING**: Replaced Makefile with justfile for task running
  - **Migration**:
    - Install mise: `brew install mise` (or see [mise installation guide](https://mise.jdx.dev/getting-started.html))
    - Install just: `mise install` (version pinned to 1.43.1 in mise.toml)
    - Use `just check` instead of `make check`
    - See `just --list` for all available commands
  - **Rationale**: Better cross-platform support, improved UX, and local/CI environment parity

### Added
- mise configuration for reproducible development environments
- Dependabot for automated dependency updates of GitHub Actions and Cargo
- Separate fast (`just test`) and CI-equivalent (`just test-ci`) commands
- Support for removing prunable worktrees (directories manually deleted)
  - `ofsht rm` can now remove worktrees even if their directories no longer exist
  - Works with branch names, absolute paths, and relative paths
  - Git marks these as "prunable" and ofsht handles them gracefully

### Security
- GitHub Actions now use SHA-pinned mise-action with fixed mise version (2025.11.5)
- Removed unnecessary `checks: write` permission from CI workflow

## [0.2.0] - TBD

### Changed - BREAKING CHANGES

#### Configuration File Structure Change
- **Breaking**: Configuration file structure has been updated for better clarity
  - **Changes**:
    - `[defaults]` section renamed to `[worktree]`
    - `[zoxide]` configuration is now `[integration.zoxide]` and only available in global config (`~/.config/ofsht/config.toml`)
    - `hooks.delete.copy` removed (use case was unclear)
  - **Migration**: Update your config files:
    ```toml
    # Before (v0.1.x)
    [defaults]
    dir = "../{repo}-worktrees/{branch}"

    [zoxide]
    enabled = true

    # After (v0.2.0)
    [worktree]
    dir = "../{repo}-worktrees/{branch}"

    # Integration config only in global config (~/.config/ofsht/config.toml)
    [integration.zoxide]
    enabled = true
    ```
  - **Reason**: More semantic naming and proper separation of global vs local settings

#### Default Path Template Change
- **Breaking**: Default path template changed from `../worktrees/{repo}/{branch}` to `../{repo}-worktrees/{branch}`
  - **Migration**: If you rely on the default template without a config file, update your `.ofsht.toml`:
    ```toml
    [worktree]
    dir = "../worktrees/{repo}/{branch}"  # Old behavior
    ```
  - **Reason**: Shorter paths with repository name more prominent

#### Relative Path Resolution
- **Breaking**: Relative paths are now resolved from the main repository root instead of the current working directory
  - **Previous behavior**: When running `ofsht add` from within a worktree, relative paths would be resolved from that worktree's location
  - **New behavior**: Relative paths are always resolved from the main repository root, regardless of where the command is executed
  - **Migration**: If you have custom path templates using relative paths, they will now be resolved from the main repo root
  - **Reason**: More predictable behavior when working from within worktrees

#### XDG Config Directory
- **Breaking**: Global config location now follows XDG Base Directory specification on all platforms
  - **Previous behavior**:
    - macOS: `~/Library/Application Support/ofsht/config.toml`
    - Linux: `~/.config/ofsht/config.toml`
  - **New behavior**: All platforms respect `XDG_CONFIG_HOME`
    - If `$XDG_CONFIG_HOME` is set (and absolute): `$XDG_CONFIG_HOME/ofsht/config.toml`
    - Otherwise: `~/.config/ofsht/config.toml`
  - **Migration for macOS users**:
    ```bash
    # If you have a global config, move it to the new location:
    mkdir -p ~/.config/ofsht
    mv ~/Library/Application\ Support/ofsht/config.toml ~/.config/ofsht/config.toml
    ```
  - **Reason**: Consistent configuration location across all platforms

#### `add` Command Behavior Change
- **Breaking**: `ofsht add` now prints absolute path to STDOUT for shell wrapper integration
  - **Previous behavior**: Printed "Created worktree at: ..." to STDOUT
  - **New behavior**: Prints only the absolute worktree path to STDOUT
  - Progress messages and hook output now go to STDERR
  - **Migration**: If you were parsing the output of `ofsht add`, update to parse the absolute path only
  - **Reason**: Enables automatic navigation with shell wrapper (see README for examples)

#### Shell Completion Command Rename
- **Breaking**: `completions` command renamed to `completion` (singular form)
  - **Previous behavior**: `ofsht completions bash`
  - **New behavior**: `ofsht completion bash`
  - **Migration**: Update scripts and documentation to use `ofsht completion` instead of `ofsht completions`
  - **Reason**: Better naming consistency with other CLI tools

### Added

- **`create` command**: New command to create worktrees without automatic navigation
  - `ofsht create <branch>` creates a worktree and keeps you in the current directory
  - Provides user-friendly messages to STDERR
  - Useful when you don't want to navigate immediately
  - `ofsht add <branch>` now outputs path for shell integration
- Integration tests for repository root detection
- Improved error messages when not in a git repository
- Support for bare repositories
- Home-relative path display with `~` for improved readability
  - `ofsht create` now shows `~/repo-worktrees/branch` instead of `/Users/user/repo-worktrees/branch`
  - Hook messages (copy, symlink) also use `~` notation
  - `ofsht ls` and `ofsht cd` remain unchanged (git output and shell compatibility)
- Shell wrapper examples for automatic navigation (Bash, Zsh, Fish)

### Technical Details

- Uses `git rev-parse --git-common-dir` to find main repository (requires Git ≥ 2.5, 2015)
- Path canonicalization for reliable symbolic link handling
- Enhanced test coverage with `assert_cmd` and `assert_fs`
- Introduced trait-based architecture for testability:
  - `GitClient` trait for git operations
  - `HookExecutor` trait for hook execution
  - `ZoxideClient` trait for zoxide integration
- Added `WorktreeService` for coordinated operations with dependency injection
- Separated STDOUT (machine-readable data) and STDERR (human-readable messages)
- Comprehensive unit tests with mock implementations

## [0.1.0] - Initial Release

### Added

- Basic worktree management commands: `add`, `ls`, `cd`, `rm`
- Hook system for worktree creation and deletion
  - Run commands
  - Copy files
  - Create symbolic links
- zoxide integration for quick navigation
- Shell completion generation (Bash, Zsh, Fish)
- Support for custom start points when creating worktrees
- Two-tier configuration system (local `.ofsht.toml` and global config)
- Path template system with `{repo}` and `{branch}` variables

[Unreleased]: https://github.com/wadackel/ofsht/compare/v0.1.0...HEAD
[0.2.0]: https://github.com/wadackel/ofsht/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/wadackel/ofsht/releases/tag/v0.1.0
