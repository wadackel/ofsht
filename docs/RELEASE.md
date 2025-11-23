# Release Process

This document describes the release process for **ofsht**.

## Overview

Releases are fully automated using [release-plz](https://release-plz.ieni.dev/) and GitHub Actions. The workflow handles:

- Version bumping based on Conventional Commits
- CHANGELOG.md generation and updates
- Publishing to crates.io
- Creating GitHub Releases with binary assets
- Building cross-platform binaries (Linux, macOS)

## Release Workflow

### Automatic Release (Current Setup)

1. **Development**: Work on features/fixes on feature branches
2. **Commit**: Use Conventional Commits format (see below)
3. **Merge to main**: Merge pull requests to the main branch
4. **Tag**: Create and push a version tag
5. **Automation**: GitHub Actions automatically:
   - Builds binaries for all platforms
   - Publishes to crates.io
   - Creates GitHub Release with assets
   - Updates CHANGELOG.md

### Triggering a Release

```bash
# Ensure main branch is up to date
git checkout main
git pull origin main

# Create a version tag (must start with 'v')
git tag v0.2.0

# Push the tag to trigger release workflow
git push origin v0.2.0
```

The `.github/workflows/release.yaml` workflow will automatically start.

## Conventional Commits

This project follows the [Conventional Commits](https://www.conventionalcommits.org/) specification for commit messages. This enables automatic changelog generation and semantic versioning.

### Commit Message Format

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

### Commit Types

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

### Breaking Changes

Breaking changes trigger a major version bump (x.0.0):

```bash
# Method 1: Use BREAKING CHANGE in footer
git commit -m "feat: redesign config file format

BREAKING CHANGE: Config file format changed from JSON to TOML"

# Method 2: Add ! after type
git commit -m "feat!: redesign config file format"
```

### Examples

```bash
# Feature addition (bumps minor version)
git commit -m "feat: add --all flag to list command"

# Bug fix (bumps patch version)
git commit -m "fix: handle empty worktree list gracefully"

# Performance improvement
git commit -m "perf: optimize worktree path resolution"

# Documentation update
git commit -m "docs: add examples to README"

# Dependency update
git commit -m "deps: update clap to 4.5.0"

# Breaking change (bumps major version)
git commit -m "feat!: change default worktree directory template"
```

### Scope (Optional)

Add scope to provide additional context:

```bash
git commit -m "feat(tmux): add pane creation support"
git commit -m "fix(config): resolve XDG_CONFIG_HOME path correctly"
git commit -m "docs(hooks): document run/copy/link actions"
```

## Version Management

This project follows [Semantic Versioning 2.0.0](https://semver.org/):

- **Major (x.0.0)**: Breaking changes (incompatible API changes)
- **Minor (0.x.0)**: New features (backward compatible)
- **Patch (0.0.x)**: Bug fixes (backward compatible)

### Pre-1.0 Versioning

While in 0.x.y versions:
- Breaking changes bump the minor version (0.x.0)
- Features bump the minor version (0.x.0)
- Bug fixes bump the patch version (0.0.x)

## Release Checklist

Before creating a release tag:

- [ ] All CI checks pass on main branch
- [ ] Manual testing completed (see `docs/TEST.md`)
- [ ] CHANGELOG.md reviewed (ensure unreleased section is ready)
- [ ] Version number determined based on changes
- [ ] Documentation updated if needed

After release:

- [ ] Verify GitHub Release created successfully
- [ ] Verify all 4 binary assets attached
- [ ] Verify crates.io publication
- [ ] Test installation: `cargo install ofsht`
- [ ] Update any external documentation/announcements

## Platform Support

Binary releases are provided for:

- **Linux**:
  - `x86_64-unknown-linux-gnu` (dynamically linked)
  - `x86_64-unknown-linux-musl` (statically linked)
- **macOS**:
  - `x86_64-apple-darwin` (Intel)
  - `aarch64-apple-darwin` (Apple Silicon)

Windows support may be added in the future.

## Rollback Procedure

### If release fails during workflow execution:

1. Check workflow logs in GitHub Actions
2. Fix the issue in a new commit
3. Delete the failed tag:
   ```bash
   git tag -d v0.x.0
   git push origin :refs/tags/v0.x.0
   ```
4. Create a new tag after fixing

### If release workflow fails at a specific step:

**During draft release creation** (Run release-plz):
- The draft release may be partially created
- Check GitHub Releases page to see if a draft exists
- If draft exists but is incomplete:
  ```bash
  # Delete the draft release
  gh release delete v0.x.0 --yes

  # Delete the tag
  git tag -d v0.x.0
  git push origin :refs/tags/v0.x.0

  # Fix the issue and retry
  ```

**During asset upload** (Upload release assets):
- The draft release exists but some/all assets are missing
- You can manually upload missing assets:
  ```bash
  # Upload missing assets to the draft
  gh release upload v0.x.0 path/to/asset.tar.gz --clobber

  # Then publish the release
  gh release edit v0.x.0 --draft=false --latest
  ```
- Or delete and retry:
  ```bash
  gh release delete v0.x.0 --yes
  git tag -d v0.x.0
  git push origin :refs/tags/v0.x.0
  ```

**After release is published**:
- With Immutable releases enabled, you CANNOT modify published releases
- You must create a new patch version

### If release completed but has critical issues:

**Note**: With Immutable releases enabled, you cannot modify or delete existing releases.

1. **Yank the version from crates.io**:
   ```bash
   cargo yank --vers 0.x.0 ofsht
   ```
   This prevents new users from installing the broken version.

2. **Create a patch release**:
   - Fix the issue
   - Commit with appropriate message
   - Tag and release the fixed version (e.g., v0.x.1)

3. **Document the issue**:
   - Note the problem in the new release notes
   - Update README/docs if necessary

## Troubleshooting

### Workflow fails at "Build release binary"

- Check Rust compilation errors in logs
- Ensure all dependencies are compatible with target platform
- Verify `Cargo.lock` is committed

### Workflow fails at "Run release-plz"

- Verify `CARGO_REGISTRY_TOKEN` secret is set and valid
- Check crates.io status: https://status.crates.io/
- Ensure version doesn't already exist on crates.io

### Workflow fails at "Upload release assets"

- Verify `RELEASE_PLZ_GITHUB_TOKEN` has correct permissions (needs `contents: write`)
- Check that artifacts were created in previous jobs
- Ensure the draft release was created successfully by release-plz
- Verify GitHub CLI (`gh`) is available in the runner

### Workflow fails at "Verify release assets"

This step checks that exactly 4 binary assets were uploaded. If it fails:

- Check the error message to see how many assets were found
- Review the "Upload release assets" step logs
- Verify all 4 build matrix jobs completed successfully
- Check artifact names match the expected pattern: `ofsht-{target}.tar.gz`

Manual verification:
```bash
gh release view v0.x.0 --json assets --jq '.assets[].name'
```

### Workflow fails at "Publish release"

- Verify the draft release exists
- Check `RELEASE_PLZ_GITHUB_TOKEN` permissions
- Ensure Immutable releases is enabled in repository settings
- Verify all assets are attached before publishing

### Release published but binaries missing

This should not happen due to the verification step, but if it does:

- **DO NOT** try to add assets to a published release (Immutable releases prevents this)
- Create a new patch version with the missing binaries
- Document the issue in the new release notes

## References

- [release-plz Documentation](https://release-plz.ieni.dev/)
- [Conventional Commits Specification](https://www.conventionalcommits.org/)
- [Semantic Versioning](https://semver.org/)
- [Keep a Changelog](https://keepachangelog.com/)
- [GitHub Immutable Releases](https://github.blog/changelog/2024-11-12-immutable-releases-general-availability/)
