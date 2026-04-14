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

- Failure mode: I repeated the same parallel-verification mistake later in the session while checking the new rooted tree browser, which wasted time and produced non-actionable lock failures.
- Detection signal: real `dump-screen` runs for `conversations` and `messages` failed with `database is locked`, while the same commands succeeded immediately when rerun one at a time.
- Prevention rule: once a verification path is known to mutate shared SQLite state, treat parallel execution as invalid by default for the rest of the session; do not retry the same mistake.

- Failure mode: I also parallelized compile/test verification after already confirming this repo's Rust build and SQLite-backed checks should be run sequentially, causing repeated lock contention and wasted cycles.
- Detection signal: multiple `cargo check` / `cargo test` commands started together reported package-cache and build-directory lock waits instead of providing new information.
- Prevention rule: for this repo, run verification serially once the first command needs the build directory or local index; use parallel reads only for inspection, not for cargo or DB-mutating commands.

- Failure mode: I treated Claude sidechain `role:"user"` records as real human user messages, which mislabeled delegated subagent prompts as `You` in the browser.
- Detection signal: a real Claude conversation showed a nested `You` prompt that matched the assistant's `Agent` tool input rather than anything the user actually typed.
- Prevention rule: when importing harness transcripts, classify authorship from transcript provenance as well as message role; sidechain or tool-delegated prompts must not be rendered as human-authored user messages by default.
