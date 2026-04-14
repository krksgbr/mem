# Lessons

## 2026-03-22

- Failure mode: I relied on crate tests and missed a warning that only showed up in a normal `cargo check` build because a test-only helper module was compiled into the main binary target.
- Detection signal: the user ran `cargo check` and surfaced a dead-code warning that my verification had not exercised.
- Prevention rule: for this repo, always run `cargo check` in addition to `cargo test -p shared` and `cargo test -p cli` before declaring the work verified.

## 2026-04-14

- Failure mode: I initially recorded interactive TUI `update` time around the input poll loop, which accidentally included the 16ms idle poll wait and misattributed the cost to state updates.
- Detection signal: the first interactive `--profile` report showed `update` dominating even though the known hot path was render work.
- Prevention rule: when profiling event loops, measure wait time and work time separately; never wrap blocking poll/sleep calls inside an application-phase timer.

- Failure mode: I ran multiple DB-backed CLI verification commands in parallel against the same SQLite index and triggered `database is locked` errors that obscured the actual rendering measurement.
- Detection signal: `profile-scroll` and `dump-screen` failed with `failed to clear contentless FTS index` / `database is locked` during parallel verification.
- Prevention rule: when verifying commands that auto-sync or mutate the shared local index, run them sequentially or point them at isolated state paths; do not parallelize them by default.
