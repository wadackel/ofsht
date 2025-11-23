# VHS documentation: https://github.com/charmbracelet/vhs
# Quick Start demo for ofsht

# === Settings (must be at the top) ===
Output quick-start.gif
Require git
Require ofsht

Set Shell "bash"
Set FontSize 28
Set Width 1400
Set Height 500
Set Theme "Dracula"
Set Padding 32
Set BorderRadius 12
Set TypingSpeed 100ms
Set PlaybackSpeed 1.4
Set LoopOffset 20%
Set MarginFill "#f5f5f5"

# === Setup (hidden from viewers) ===
Hide
Type "set -euo pipefail"
Enter
Type 'PROJECT_ROOT=$(pwd)'
Enter
Type 'export PATH="$PROJECT_ROOT/target/release:$PATH"'
Enter
Type "rm -rf /tmp/ofsht-demo && mkdir -p /tmp/ofsht-demo && cd /tmp/ofsht-demo"
Enter
Type "git init >/dev/null 2>&1"
Enter
Type "git config user.name 'Demo' && git config user.email 'demo@example.com'"
Enter
Type "echo '# Demo Project' > README.md"
Enter
Type "git add README.md && git commit -m 'Initial commit' >/dev/null 2>&1"
Enter

# Create .ofsht.toml for hooks demo
Type "cat > .ofsht.toml << 'EOF'"
Enter
Type "[hooks.create]"
Enter
Type 'run = ["echo \"✓ Hook executed!\""]'
Enter
Type 'copy = ["README.md"]'
Enter
Type "EOF"
Enter

Type "clear"
Enter
Show

# === Demo (visible to viewers) ===
Type "# ofsht - Git worktree manager"
Enter
Sleep 1s
Enter

Type "# Create a worktree"
Enter
Type "ofsht add feature-awesome"
Enter
Sleep 2s
Enter

Type "# List all worktrees"
Enter
Type "ofsht ls"
Enter
Sleep 2s
Enter

Type "# Show hook configuration"
Enter
Type "cat .ofsht.toml"
Enter
Sleep 2.5s
Enter

Type "# Create with hooks"
Enter
Type "ofsht add feature-hooks"
Enter
Sleep 2.5s
Enter

Type "# Clean up"
Enter
Type "ofsht rm feature-awesome feature-hooks"
Enter
Sleep 2s

# Final pause before loop
Sleep 3s
