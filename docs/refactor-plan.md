# Transcript Browser Refactor Plan

## Goals

- Keep the existing TUI working while removing spike-era coupling.
- Establish a normalized transcript domain that supports Claude Code and Codex now.
- Isolate provider parsing so adding more providers is localized work.
- Improve the test surface before adding richer browsing features.

## Target Structure

### `shared`

- `shared/src/domain.rs`
  Normalized transcript types only.
- `shared/src/app.rs`
  App state, events, navigation, filters, and view-model derivation.
- `shared/src/lib.rs`
  Public exports only.

### `cli`

- `cli/src/main.rs`
  Thin composition root.
- `cli/src/runtime.rs`
  Terminal setup, teardown, event loop, redraw loop.
- `cli/src/input.rs`
  Key mapping from crossterm events to app events.
- `cli/src/theme.rs`
  Theme definitions.
- `cli/src/render/mod.rs`
  Top-level render dispatch.
- `cli/src/render/table.rs`
  Table rendering.
- `cli/src/render/messages.rs`
  Message list rendering.
- `cli/src/render/split.rs`
  Split-pane rendering.
- `cli/src/test_utils.rs`
  Shared render test helpers.

### Provider Parsing

Start inside `cli` to keep scope small. If this grows further, move it into a dedicated crate.

- `cli/src/providers/mod.rs`
  Provider registry and shared loading helpers.
- `cli/src/providers/claude.rs`
  Claude discovery and parsing.
- `cli/src/providers/codex.rs`
  Codex discovery and parsing.
- `cli/src/providers/common.rs`
  Shared parsing helpers.

## Domain Direction

The normalized domain should stay richer than `recall`'s flatter session model.

Current center:

- `Workspace`
- `Conversation`
- `Message`
- `ProviderKind`
- `Participant`

Planned refinements:

- Make `Workspace` naming deliberate:
  likely keep `Workspace` unless another provider proves `Project` or `Repository` is more accurate.
- Keep `external_id` separate from internal stable IDs.
- Preserve provider-specific metadata only at the boundary unless it becomes a real product concept.
- Keep view-specific preview structs out of `domain.rs`.

## Reference Project

Use `/Users/gaborkerekes/playground/ai-stuff/harness-utils/session-managers/recall` as a reference for:

- Claude and Codex transcript shapes
- parser module boundaries
- transcript cleanup rules
- Ratatui test patterns

Do not copy its domain model wholesale. This app needs richer browsing than `recall`'s session-search focus.

## Thin Slices

### Slice 1: Extract Claude Provider

Scope:

- Move Claude discovery/parsing out of `cli/src/main.rs`.
- Keep current behavior unchanged.
- Add parser-focused tests using fixtures or minimal inline samples.

Files:

- add `cli/src/providers/mod.rs`
- add `cli/src/providers/claude.rs`
- trim `cli/src/main.rs`

Verification:

- `cargo test -p shared`
- `cargo test -p cli`

### Slice 2: Add Codex Provider

Scope:

- Parse Codex sessions from `~/.codex/sessions`.
- Normalize into the existing domain model.
- Filter injected AGENTS/environment blocks similarly to `recall`.

Files:

- add `cli/src/providers/codex.rs`
- expand provider registry and loader wiring
- add parser tests

Verification:

- `cargo test -p shared`
- `cargo test -p cli`

### Slice 3: Refactor App State

Scope:

- Replace global selection fields with screen-specific state.
- Remove duplicated reset/bounds logic.
- Keep view output stable where possible.

Files:

- `shared/src/app.rs`
- tests in `shared/src/app.rs`

Verification:

- `cargo test -p shared`
- `cargo test -p cli`

### Slice 4: Split CLI Runtime And Render

Scope:

- Extract runtime/input/render modules from `cli/src/main.rs`.
- Add a terminal lifecycle guard so raw mode is restored on failure.
- Stop swallowing key load/runtime errors silently where they matter.

Files:

- `cli/src/main.rs`
- add `cli/src/runtime.rs`
- add `cli/src/input.rs`
- add `cli/src/theme.rs`
- add `cli/src/render/*`

Verification:

- `cargo test -p cli`

### Slice 5: Replace Spike Debris With Real Fixtures

Scope:

- Remove scratch tests that only print or write files.
- Move useful sample data under fixture directories.
- Add assertions for parser edge cases and render output.

Files:

- `cli/src/test_compare.rs`
- `cli/src/test_large.rs`
- `cli/src/test_textwrap.rs`
- root-level scratch artifacts and helper scripts, if no longer needed

Verification:

- `cargo test -p shared`
- `cargo test -p cli`

## Proposed Execution Order

1. Extract Claude provider.
2. Add Codex provider.
3. Refactor shared app state.
4. Split CLI runtime/render.
5. Clean tests and spike debris.
6. Start feature work on top of the cleaned structure.

## Risks To Watch

- Do not let provider-specific raw transcript shapes leak into `shared`.
- Do not use the current TUI loop as the place for filesystem and parsing logic long term.
- Do not over-abstract provider loading before the Claude and Codex modules both exist.
- Do not keep scratch tests as the only “documentation” of rendering behavior.

## Source Of Truth

- Never run `cargo run` for verification.
- Use `cargo test -p shared` for app-state behavior.
- Use `cargo test -p cli` for rendering and integration at the crate level.
