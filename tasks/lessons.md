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

- Failure mode: I parallelized a manual `sqlite3` inspection against the same local index while another DB-backed verification command was active, and hit `database is locked` again.
- Detection signal: one of two concurrent inspection commands failed immediately with `Error: in prepare, database is locked (5)` while the equivalent serial command worked.
- Prevention rule: in this repo, treat *all* access to the shared SQLite index as serial during verification, including ad hoc `sqlite3` reads; do not assume a manual read is safe to parallelize just because it looks read-only.

- Failure mode: I initially collapsed Claude delegation sidechains by grouping only root rows, which left structural child replies unvisited and leaked subagent transcript messages back into the top-level conversation tree.
- Detection signal: the `burst-perception-integration` tree showed delegated task nodes followed by stray subagent assistant lines at the same visual level.
- Prevention rule: when collapsing a tree into summary rows, mark the full descendant subtree covered by the summary, not just the root nodes that triggered the collapse.

- Failure mode: I made browser visibility of real forked child conversations depend on successful branch-anchor message resolution inside the parent transcript, which caused valid children to disappear entirely when the anchor message was missing or unmatched.
- Detection signal: `cc-sessions` showed `research-building-a-user-model` with child forks, but transcript-browser showed the row as non-expandable or empty on expand.
- Prevention rule: in the conversation browser, real forked child conversations must remain visible from parent→child session lineage alone; anchor-message placement is an optional refinement, not a prerequisite for visibility.

- Failure mode: I checked for a newly added skill only in the global user skill directory and missed the repo-local skill the user had pointed me to.
- Detection signal: the user explicitly said the skill was "here in this repo" after I incorrectly reported it missing.
- Prevention rule: when a user points to a skill path, resolve it relative to the current repository before assuming it should exist in the global skill directories.

- Failure mode: I changed how conversation titles/previews are derived from transcript content, but left the rebuildable SQLite index at the same schema version, so the browser kept serving stale conversation summaries while `read` recomputed the new title on demand.
- Detection signal: the same conversation rendered as `/login` in `read`, but still appeared as a raw internal conversation ID in the conversations browser.
- Prevention rule: when changing any persisted derivation logic that affects indexed summaries or query-visible fields, bump the SQLite schema/index version so the derived store is rebuilt instead of silently mixing old indexed data with new runtime logic.

- Failure mode: I initially treated `tui-use` as broken when the real blocker was sandbox access to `~/.tui-use`, which the tool needs for its daemon socket and session state.
- Detection signal: `tui-use start` failed with `EPERM: operation not permitted, mkdir '/Users/gaborkerekes/.tui-use'`, and `nono why` reported `path_not_granted` for write access to that path.
- Prevention rule: when evaluating local tooling that maintains daemon or session state under the home directory, check its state path and sandbox access first; do not diagnose higher-level tool behavior until that prerequisite is verified.

- Failure mode: I navigated `transcript-browser` under `tui-use` by raw key counts, which made me inspect the wrong workspace/conversation and reduced the value of the captured snapshots.
- Detection signal: the PTY snapshots were readable and highlighted the selected row correctly, but the later interaction sequence landed in `~/.config/konfigue` instead of the intended bookmarking workspace.
- Prevention rule: use `tui-use` as an exploratory interaction tool, but anchor scripted navigation to visible text or explicit searchable state rather than relative key counts whenever the target matters.

- Failure mode: applying `Event::SetWorkspaces` after background sync unconditionally reset the app to `Screen::Workspaces { selected_workspace: 0 }`, which made the UI "bounce" back to home after a delayed refresh.
- Detection signal: the user consistently observed the app returning to the workspace screen after roughly the background-sync completion window, and the core update handler showed `SetWorkspaces` hard-resetting the screen regardless of current navigation state.
- Prevention rule: background data refresh events must preserve current navigation context by stable workspace/conversation/message identity whenever possible; never treat a snapshot refresh as an implicit navigation reset.
