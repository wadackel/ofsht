# ofsht Manual Testing Procedures

This document describes the procedures for manually verifying each feature of ofsht.

> [!NOTE]
> **Path Display Conventions:**
> - ofsht displays paths under the home directory using `~` notation
> - This document uses `/tmp` for testing, so examples show absolute paths
> - When testing under your home directory, paths will display as `~/demo-ofsht-worktrees/...`

## Prerequisites

- Rust and cargo installed
- Git installed
- (Optional) zoxide installed

## Build

```bash
# Release build
cargo build --release

# Binary path after build
# target/release/ofsht
```

## Basic Functionality Verification

### 1. Prepare Test Repository

```bash
cd /tmp
rm -rf demo-ofsht
mkdir demo-ofsht
cd demo-ofsht
git init
git commit --allow-empty -m "Initial commit"
```

### 2. Create Worktree (`ofsht add`)

#### 2-1. Basic Creation (from HEAD)

```bash
# Create new worktree from HEAD
ofsht add feature-awesome

# Expected output:
# Created worktree at: /private/tmp/demo-ofsht-worktrees/feature-awesome
```

Verify created directory:

```bash
ls -la ../demo-ofsht-worktrees/feature-awesome
```

#### 2-2. Create with Start Point Branch

First, create a test branch:

```bash
cd /tmp/demo-ofsht
git checkout -b develop
echo "develop branch" > develop.txt
git add develop.txt
git commit -m "Add develop.txt"
git checkout main
```

Create worktree with start point:

```bash
# Create new feature branch from develop branch
ofsht add feature-from-develop develop

# Expected output:
# Created worktree at: /private/tmp/demo-ofsht-worktrees/feature-from-develop

# Verify: develop.txt should exist
ls ../demo-ofsht-worktrees/feature-from-develop/
cat ../demo-ofsht-worktrees/feature-from-develop/develop.txt
```

#### 2-3. Create from Remote Branch

```bash
# Assuming remote branch (use local branch as substitute for testing)
ofsht add feature-from-remote origin/main
# Or from local branch:
ofsht add feature-from-local main
```

#### 2-4. Create from Tag

```bash
# Create tag
cd /tmp/demo-ofsht
git tag v1.0.0
git checkout main

# Create worktree from tag
ofsht add release-prep v1.0.0

# Verify
cd ../demo-ofsht-worktrees/release-prep
git log --oneline -1
```

#### 2-5. Create from Relative Commit

```bash
# Create from 3 commits before HEAD
ofsht add feature-old-commit HEAD~3

# Or from specific commit hash
# ofsht add feature-specific abc123
```

#### 2-6. Error Case: Non-existent Branch

```bash
# Specify non-existent branch
ofsht add feature-invalid does-not-exist

# Expected behavior:
# Git error message is displayed
# Error: git worktree add failed: ...
```

#### 2-7. Execute from Within Worktree (Verify Repository Root Resolution)

```bash
# Move to first worktree
cd ../demo-ofsht-worktrees/feature-awesome

# Create new worktree from within this worktree
ofsht add feature-from-worktree

# Expected behavior:
# Relative path is resolved from main repository root, so
# created at ../demo-ofsht-worktrees/feature-from-worktree

# Verify
ls -la ../feature-from-worktree

# Return to main repository
cd /tmp/demo-ofsht
```

### 3. List Worktrees (`ofsht ls`)

```bash
ofsht ls

# Expected output:
# /private/tmp/demo-ofsht                            <commit> [main]
# /private/tmp/demo-ofsht-worktrees/feature-awesome  <commit> [feature-awesome]
```

### 4. Navigate to Worktree (`ofsht cd`)

```bash
# Get worktree path
ofsht cd feature-awesome

# Expected output:
# /private/tmp/demo-ofsht-worktrees/feature-awesome

# To actually navigate
cd $(ofsht cd feature-awesome)
pwd
```

### 5. Remove Worktree (`ofsht rm`)

Create another worktree before removing:

```bash
# Create worktree for deletion
ofsht add test-delete

# Remove worktree
ofsht rm /private/tmp/demo-ofsht-worktrees/test-delete

# Expected output:
# Removed worktree: /private/tmp/demo-ofsht-worktrees/test-delete
# Deleted branch: test-delete

# Verify
ofsht ls
```

### 6. Remove Prunable Worktree

Test removing worktrees whose directories have been manually deleted (prunable state).
When a worktree directory is deleted manually, Git marks it as "prunable" and `ofsht rm` can still remove it:

```bash
# Create a worktree
ofsht add test-prunable

# Manually delete the worktree directory to make it prunable
rm -rf ../demo-ofsht-worktrees/test-prunable

# Verify it's shown as prunable (use --porcelain for clear output)
git worktree list --porcelain
# Expected: Shows a line containing "prunable" after the test-prunable entry

# Remove by branch name
ofsht rm test-prunable
# Expected output:
# Removed worktree: /path/to/demo-ofsht-worktrees/test-prunable
# Deleted branch: test-prunable

# Verify removal
git worktree list --porcelain | grep test-prunable
# Expected: No output (entry removed)
git branch --list test-prunable
# Expected: Empty (branch deleted)

# Test with absolute path
ofsht add test-prunable-abs
rm -rf ../demo-ofsht-worktrees/test-prunable-abs
# Get absolute path
ABS_PATH=$(cd ../demo-ofsht-worktrees && pwd)/test-prunable-abs
ofsht rm "$ABS_PATH"
# Expected output:
# Removed worktree: /path/to/demo-ofsht-worktrees/test-prunable-abs
# Deleted branch: test-prunable-abs

# Verify removal
git worktree list --porcelain | grep test-prunable-abs
# Expected: No output
git branch --list test-prunable-abs
# Expected: Empty

# Test with relative path
ofsht add test-prunable-rel
rm -rf ../demo-ofsht-worktrees/test-prunable-rel
ofsht rm ../demo-ofsht-worktrees/test-prunable-rel
# Expected output:
# Removed worktree: /path/to/demo-ofsht-worktrees/test-prunable-rel
# Deleted branch: test-prunable-rel

# Verify removal
git worktree list --porcelain | grep test-prunable-rel
# Expected: No output
git branch --list test-prunable-rel
# Expected: Empty

# Final verification - all prunable worktrees should be removed
git worktree list
# Expected: Only shows main worktree and any other active worktrees
```

## Hook Functionality Verification

### 1. Create Configuration File

```bash
cd /tmp/demo-ofsht

cat > .ofsht.toml << 'EOF'
[hooks.create]
run = ["echo 'Hello from create hook!'", "pwd"]
copy = []
link = {}

[hooks.delete]
run = ["echo 'Goodbye from delete hook!'"]

[worktree]
dir = "../{repo}-worktrees/{branch}"
EOF
```

### 2. Verify Create Hook

```bash
ofsht add feature-with-hooks

# Expected output:
# Created worktree at: /private/tmp/demo-ofsht-worktrees/feature-with-hooks
# Executing create hooks...
# Hello from create hook!
# /private/tmp/demo-ofsht-worktrees/feature-with-hooks
```

### 3. Verify Copy and Link

```bash
# Create test file
echo "test config" > .testrc

cat > .ofsht.toml << 'EOF'
[hooks.create]
run = []
copy = [".testrc"]
link = { ".testrc" = ".testrc-link" }

[worktree]
dir = "../{repo}-worktrees/{branch}"
EOF

# Create worktree
ofsht add feature-copy-link

# Verify copied file
cat ../demo-ofsht-worktrees/feature-copy-link/.testrc

# Verify symlink
ls -la ../demo-ofsht-worktrees/feature-copy-link/.testrc-link
cat ../demo-ofsht-worktrees/feature-copy-link/.testrc-link
```

### 4. Verify Delete Hook

```bash
cat > .ofsht.toml << 'EOF'
[hooks.delete]
run = ["echo 'Cleaning up...'", "ls -la"]
EOF

ofsht rm /private/tmp/demo-ofsht-worktrees/feature-copy-link

# Expected output:
# Executing delete hooks...
# Cleaning up...
# (directory listing)
# Removed worktree: /private/tmp/demo-ofsht-worktrees/feature-copy-link
# Deleted branch: feature-copy-link
```

## zoxide Integration Verification

### Verify Prerequisites

```bash
# Check if zoxide is installed
zoxide --version

# If not installed:
# macOS: brew install zoxide
# Linux: curl -sS https://raw.githubusercontent.com/ajeetdsouza/zoxide/main/install.sh | bash
```

### 1. Verify with zoxide Enabled (Default)

```bash
cd /tmp/demo-ofsht

# Remove config file (use default settings)
rm -f .ofsht.toml

# Create worktree
ofsht add feature-with-zoxide

# Verify registration in zoxide
zoxide query feature-with-zoxide

# Expected output:
# /private/tmp/demo-ofsht-worktrees/feature-with-zoxide
```

### 2. Verify with zoxide Disabled

```bash
# Create config file to disable zoxide
cat > .ofsht.toml << 'EOF'
[zoxide]
enabled = false
EOF

# Create worktree
ofsht add feature-no-zoxide

# Verify not registered in zoxide
zoxide query feature-no-zoxide 2>&1 || echo "✅ Not registered in zoxide (as expected)"

# Expected output:
# zoxide: no match found
# ✅ Not registered in zoxide (as expected)
```

### 3. Practical Workflow with zoxide

```bash
# Remove config file (re-enable zoxide)
rm -f .ofsht.toml

# Create new worktree
ofsht add feature-user-auth

# Navigate quickly with zoxide
z feature-user-auth
pwd

# Expected output:
# /private/tmp/demo-ofsht-worktrees/feature-user-auth
```

## Path Template Customization Verification

### Configure Custom Path Template

```bash
cd /tmp/demo-ofsht

cat > .ofsht.toml << 'EOF'
[worktree]
dir = "/tmp/custom-worktrees/{repo}/{branch}"
EOF

# Create worktree with custom path
ofsht add feature-custom-path

# Verify
ls -la /tmp/custom-worktrees/demo-ofsht/feature-custom-path
ofsht ls

# Cleanup
rm -rf /tmp/custom-worktrees
```

## Global Configuration Verification

### Create Global Configuration File

```bash
# Create global config directory
mkdir -p ~/.config/ofsht

cat > ~/.config/ofsht/config.toml << 'EOF'
[worktree]
dir = "~/worktrees/{repo}/{branch}"

[zoxide]
enabled = true

[hooks.create]
run = ["echo 'Global hook executed!'"]
EOF
```

### Verify Local Configuration Priority

```bash
cd /tmp/demo-ofsht

# Without local config, global config is used
rm -f .ofsht.toml
ofsht add feature-global

# Expected output includes "Global hook executed!"

# Create local config
cat > .ofsht.toml << 'EOF'
[hooks.create]
run = ["echo 'Local hook executed!'"]
EOF

# Local config takes priority
ofsht add feature-local

# Expected output includes "Local hook executed!"
# "Global hook executed!" is not executed
```

## Cleanup

After verification is complete, clean up test directories:

```bash
# Remove test directories
rm -rf /tmp/demo-ofsht
rm -rf /tmp/demo-ofsht-worktrees
rm -rf ~/demo-ofsht-worktrees

# Remove global config (if needed)
rm -rf ~/.config/ofsht
```

## Troubleshooting

### Worktree Creation Fails

```bash
# Verify it's a Git repository
git status

# Check if branch already exists
git branch -a
```

### Not Registered in zoxide

```bash
# Check if zoxide is installed
which zoxide

# Verify zoxide is enabled in config file
cat .ofsht.toml
```

### Hooks Not Executing

```bash
# Verify config file syntax
cat .ofsht.toml

# Check if command exists in PATH
which <command>
```

## Summary

This document verified the following features:

- ✅ Basic operations (add, ls, cd, rm)
- ✅ Hook functionality (create, delete)
- ✅ File copying and symlink creation
- ✅ zoxide integration
- ✅ Path template customization
- ✅ Local/global configuration

All features can be verified to work as expected.
