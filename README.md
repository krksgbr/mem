# transcript-browser

A tiny Rust workspace: **`shared`** (state/domain) + **`cli`** (Ratatui UI).

## Run

- `cargo run -p cli --` → interactive TUI (`mem`).
- `cargo run -p cli -- <command>` for one-shot CLI actions.
- Binary name is `mem` (`--bin mem`).

## CLI

- `mem workspaces [--provider claude-code|codex] [--json]`
- `mem latest [--provider claude-code|codex] [--workspace <id|name|path>] [--limit N] [--json]`
- `mem search <query> [--limit N] [--json]`
- `mem read <conversation-id-or-title> [--offset N] [--limit N] [--json]`
- `mem run --profile` to emit a profile report.

## Offline rendering helpers

- `mem dump-screen --screen-ref <file>` replays a saved screen.
- `mem dump-screen --screen messages --workspace <...> --conversation <...> [--width W --height H]` renders one frame and exits.

## Verification

- `cargo test -p shared`
- `cargo test -p cli`
- `cargo check`

## Data

Index stored at: `~/.local/state/transcript-browser/index.sqlite3` (or `$XDG_STATE_HOME/transcript-browser/index.sqlite3`).
