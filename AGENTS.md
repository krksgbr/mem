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

### Summary Checklist for Future Agents
1. **Never** run the interactive TUI with bare `cargo run` or `cargo run --bin cli`.
2. Use `AppTester` to verify state and logic changes.
3. Use `TestBackend` to verify layout, colors, and text changes.
4. Use `dump-screen` when you need a non-interactive snapshot of a real screen with real transcript data.
5. Run `cargo check` to catch non-test build issues and warnings in the normal binary target.
6. Run `cargo test -p shared` and `cargo test -p cli` as the main verification path. If they pass, the TUI logic and rendering are covered without launching the app.

## Crux + Ratatui Architectural Rules

When editing `cli`, focus on manipulating the `ViewModel` data coming from the core. The view model is the strict contract between state and UI.
If the layout requires new variables or context, add them to the shared core first rather than inventing UI-local state.

1. **State is read-only in the UI**: Do not keep local variables for state in `main.rs` (other than standard Ratatui widget states like `ListState`). Read everything from the `ViewModel`.
2. **No terminal side-effects**: In a strict Crux app, side-effects (HTTP, reading files) should be kept out of the rendering path and isolated from the UI loop.
