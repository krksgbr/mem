# AI Agent Development Guide: Crux + Ratatui

Welcome, fellow AI agent! If you are reading this, you are likely tasked with modifying or extending this Terminal UI (TUI) application. 

**CRITICAL WARNING:** Do **NOT** attempt to verify your work by running `cargo run --bin cli` or executing the compiled binary directly using a shell command tool. The TUI runs an infinite `loop {}` waiting for terminal keystrokes via `crossterm`. It will block your tool execution indefinitely, hang your session, and force the user to manually cancel your tool call.

Instead, follow this specific **Iterate-Test Workflow** designed for autonomous verification.

## Architecture Overview

This application uses a strict Model-View-Update (Elm Architecture) pattern via [Crux](https://redbadger.github.io/crux/) and [Ratatui](https://ratatui.rs/).
It is split into two crates:
1. **`shared` (The Core)**: Pure, synchronous Rust. Contains the State (`Model`), Events, and Update logic. Emits a `ViewModel`. **Zero terminal logic.**
2. **`cli` (The Shell)**: The Ratatui rendering layer. Translates the `ViewModel` into terminal widgets and forwards keystrokes to the Core.

The normalized transcript domain lives in `shared/src/domain.rs`. Keep provider-specific parsing and filesystem discovery out of that layer.

## Reference Project

There is a related local project at `/Users/gaborkerekes/playground/ai-stuff/harness-utils/session-managers/recall`.

Use it as a reference for:
- Claude and Codex transcript shapes
- provider-specific parser structure
- Ratatui testing patterns

Do **not** treat `recall` as this app's target architecture. `recall` is optimized around search/resume over flat sessions; this app is expected to grow richer browsing functionality and should keep its own domain model.

## The Iterate-Test Workflow

To safely implement new features and prove they work without launching the TUI, split your work into two phases and rely strictly on unit tests:

### Phase 1: Test the Core Logic (Behavior)
If you need to add a new feature (like a new sorting method, a new screen, or an API call), do it in the `shared` crate.

You must exhaustively test state transitions without a UI using `crux_core::testing::AppTester`.

**Reference:** Look at the `tests` module at the bottom of `shared/src/app.rs` for examples of how to instantiate the `AppTester`, send events, and assert against the resulting `ViewModel`. 
Run `cargo test -p shared` to verify.
Run `cargo check` as a cheap whole-workspace sanity check before concluding the change is done.

Also inspect `shared/src/domain.rs` before changing provider support or transcript normalization.

### Phase 2: Test the Rendering (Visuals)
If you need to change how the UI looks in the `cli` crate, **do not run the app to check your work**. 

Instead, use Ratatui's `TestBackend`. This creates a virtual terminal buffer in memory, renders your UI to it, and lets you assert the exact text placement.

For real transcript data without launching the interactive TUI, use the non-interactive dump command:

`cargo run -p cli -- dump-screen --screen <workspaces|conversations|messages> ...`

This path is safe for agents because it renders once and exits. Prefer it when you need to inspect how a real workspace/conversation renders at a specific terminal size.

**Reference:** When writing rendering tests, do not look for code snippets here. Instead, refer to existing test implementations in the `cli` crate (if available) or standard `ratatui::backend::TestBackend` patterns where you inject a mocked `ViewModel`, call the rendering function, and verify output against the buffer.
Run `cargo test -p cli` to verify.

### Additional Verification Tools

Use the tools below in this order of preference:

1. **Unit tests and render tests first**
   - `cargo test -p shared`
   - `cargo test -p cli`
   - `cargo check`
   These are the primary verification path.

2. **`dump-screen` for deterministic real-data snapshots**
   - `cargo run -p cli -- dump-screen --screen <workspaces|conversations|messages> ...`
   Use this when you need to inspect one exact screen with real indexed data, specific width/height, and controlled selection inputs.
   Prefer this over interactive runs whenever a one-shot render is enough.

3. **Screen refs for reproducible collaboration**
   - In the interactive TUI, press `C` to save a screen ref to `./transcript-browser-screen-ref.json`.
   - Replay it with:
     - `cargo run -p cli -- dump-screen --screen-ref ./transcript-browser-screen-ref.json`
   Use this when a human sees a rendering issue and wants the agent to reconstruct the same view without describing the entire navigation path manually.

4. **`--profile` for event-driven interactive diagnostics**
   - `cargo run -p cli -- --profile`
   - or `just run --profile`
   This writes `./transcript-browser-profile.json` and records startup, background sync, and interaction-driven timing/state information.
   Use it for slow renders, scroll issues, and state transitions that only appear during live use.

5. **`tui-use` for exploratory PTY interaction**
   - Example workflow:
     - `tui-use start --cwd /Users/gaborkerekes/projects/transcript-browser --cols 100 --rows 28 --label transcript-browser "just run --profile"`
     - `tui-use snapshot --format json`
     - `tui-use press arrow_down`
     - `tui-use press enter`
     - `tui-use kill`
   Use `tui-use` as a secondary exploration/repro tool when you need to drive the real TUI and inspect how it changes after keypresses.
   Do **not** treat it as the primary verification path; convert useful findings back into a screen ref, `dump-screen` repro, or test.

### Tool Selection Guidance

- If the bug is a pure state transition problem: use `AppTester` in `shared`.
- If the bug is a pure rendering/layout problem: use `TestBackend` in `cli`.
- If the bug depends on real transcript/indexed data but only needs one screen: use `dump-screen`.
- If a human wants to show the agent an exact view they saw: use a screen ref.
- If the bug appears only across live interaction over time: use `--profile`, and optionally `tui-use` for PTY driving.

### Practical Caveats

- `dump-screen`, `search`, `read`, `profile-scroll`, and SQLite-backed inspection commands share the local index. Run them **serially**, not in parallel, to avoid `database is locked` failures.
- `tui-use` is useful for exploration, but scripted navigation by raw key counts is fragile. Prefer visible-text anchors or replayable screen refs when the target screen matters.
- Interactive profiling and PTY tools are for diagnosis. Tests plus `dump-screen` remain the source of truth for repeatable verification.

### Summary Checklist for Future Agents
1. **Never** run the interactive TUI with bare `cargo run` or `cargo run --bin cli`.
2. Use `AppTester` to verify state and logic changes.
3. Use `TestBackend` to verify layout, colors, and text changes.
4. Use `dump-screen` when you need a non-interactive snapshot of a real screen with real transcript data.
5. Use `C` screen refs and `--profile` when debugging human-reported interactive issues.
6. Use `tui-use` only as an exploration/repro tool, not as the primary verification path.
7. Run `cargo check` to catch non-test build issues and warnings in the normal binary target.
8. Run `cargo test -p shared` and `cargo test -p cli` as the main verification path. If they pass, the TUI logic and rendering are covered without launching the app.

## Crux + Ratatui Architectural Rules

When editing `cli`, focus on manipulating the `ViewModel` data coming from the core. The view model is the strict contract between state and UI.
If the layout requires new variables or context, add them to the shared core first rather than inventing UI-local state.

1. **State is read-only in the UI**: Do not keep local variables for state in `main.rs` (other than standard Ratatui widget states like `ListState`). Read everything from the `ViewModel`.
2. **No terminal side-effects**: In a strict Crux app, side-effects (HTTP, reading files) should be kept out of the rendering path and isolated from the UI loop.
