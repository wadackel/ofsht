# Default target: run all checks (CI-equivalent)
default: check

# Aliases for convenience
alias h := help
help:
    @just --list

# === Quality checks (CI-equivalent, strict) ===

# Run all checks (same as CI)
check: fmt-ci clippy-ci test-ci
    @echo "✓ All checks passed (CI-equivalent)"

# === Fast development commands ===

build:
    cargo build

test:
    cargo test

fmt:
    cargo fmt -- --check

clippy:
    cargo clippy -- -D warnings

# === CI-equivalent commands (strict) ===

# Format check (CI-equivalent)
fmt-ci:
    cargo fmt --all -- --check

# Clippy lint (CI-equivalent)
clippy-ci:
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Run tests (CI-equivalent)
test-ci:
    cargo test --workspace --all-targets --locked --no-fail-fast

# === Utility commands ===

install:
    cargo install --path .

clean:
    cargo clean

# === VHS demo commands ===

# Generate a specific demo (e.g., just demo quick-start)
demo name:
    cargo build --release
    cd docs/assets/vhs && vhs {{name}}.tape

# Generate all demo GIFs
demo-all:
    #!/usr/bin/env bash
    set -euo pipefail
    cargo build --release
    cd docs/assets/vhs
    for tape in *.tape; do
        echo "Generating $(basename "$tape" .tape)..."
        vhs "$tape"
    done
    echo "✓ All demos generated"

# Validate tape files without generating (dry-run)
demo-verify:
    #!/usr/bin/env bash
    set -euo pipefail
    cd docs/assets/vhs
    for tape in *.tape; do
        echo "Validating $tape..."
        vhs validate "$tape"
    done
    echo "✓ All tape files valid"
