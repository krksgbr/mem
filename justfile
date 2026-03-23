set shell := ["zsh", "-cu"]

default: list

list:
  @just --list

# Whole-workspace sanity check for the normal binary targets.
check:
  cargo check

fmt:
  cargo fmt

test-shared:
  cargo test -p shared

test-cli:
  cargo test -p cli

# Standard verification flow for this repo.
verify: fmt check test-shared test-cli

beans:
  beans list --config .beans.yml --json

# Manual-only: starts the interactive TUI.
# Agents should not run this for verification because it blocks waiting for terminal input.
run *args:
  cargo run -p cli -- {{args}}

# Non-interactive screen dump for a real workspace/view.
dump-screen *args:
  cargo run -p cli -- dump-screen {{args}}
