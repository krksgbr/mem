# Rendering Implementation Advice

This note synthesizes the most relevant rendering lessons from:

- [`recall`](docs/research/transcript-indexers/2026-03-23-recall.md)
- local `recall` code under:
  - [`recall/src/ui.rs`](/Users/gaborkerekes/playground/ai-stuff/harness-utils/session-managers/recall/src/ui.rs)
  - [`recall/src/app.rs`](/Users/gaborkerekes/playground/ai-stuff/harness-utils/session-managers/recall/src/app.rs)
- local Pi code under:
  - [`tree-selector.ts`](/Users/gaborkerekes/git/pi-mono/packages/coding-agent/src/modes/interactive/components/tree-selector.ts)
  - [`session-selector.ts`](/Users/gaborkerekes/git/pi-mono/packages/coding-agent/src/modes/interactive/components/session-selector.ts)
  - [`AgentInterface.ts`](/Users/gaborkerekes/git/pi-mono/packages/web-ui/src/components/AgentInterface.ts)
  - [`MessageList.ts`](/Users/gaborkerekes/git/pi-mono/packages/web-ui/src/components/MessageList.ts)
  - [`Messages.ts`](/Users/gaborkerekes/git/pi-mono/packages/web-ui/src/components/Messages.ts)

The goal is not to copy either project. The goal is to extract practical rendering patterns for `transcript-browser`.

## Current Problem

The current transcript renderer in [`cli/src/render.rs`](/Users/gaborkerekes/projects/transcript-browser/cli/src/render.rs) rebuilds and wraps the full transcript on every interaction.

That causes two problems:

- transcript scroll can feel visually confusing unless selection is very explicit
- transcript interactions are too slow because render cost dominates

Recent profiling showed scroll interactions spending the vast majority of their time in render, not in update or hydration.

## What Recall Gets Right

`recall` uses a preview renderer that is conceptually simple and effective:

- it parses a session into messages
- it builds a flat `Vec<Line>` for the preview
- it tracks `message_start_lines`
- it keeps an explicit `preview_scroll` line offset in app state
- it renders only the visible slice by skipping `preview_scroll`

The important ideas are:

1. **Separate message focus from line scroll**
   - `focused_message` identifies the conceptual message
   - `preview_scroll` identifies the visual line offset

2. **Track line boundaries while rendering**
   - `message_start_lines`
   - `message_line_ranges`

3. **Compute viewport position after wrapping is known**
   - scrolling to a message is resolved during render, because only render knows wrapped line counts

4. **Render only the visible slice**
   - not the full transcript body

This is a good fit for transcript-browser’s current transcript view.

## What Pi Gets Right

Pi has two rendering patterns that matter:

- live transcript rendering is message-level
- history browsing is tree-level

The most useful part for transcript-browser is the history/tree browser pattern:

- build structured nodes with parent/child relationships
- flatten that structure into a visible list
- preserve selection by stable ID
- render only a window around the selected node

The important ideas are:

1. **Flatten before rendering**
   - renderers should consume a flat visible list, not recursively walk the tree live

2. **Preserve selection by ID, not index**
   - this matters when folding, filtering, or syncing changes the visible ordering

3. **Track active path explicitly**
   - useful for tree/history browsers, less critical for the current linear transcript view

4. **Keep metadata browsing separate from full transcript payloads**
   - browser views should read light index data first
   - full transcript content should load only when needed

This is a good fit for transcript-browser’s future Pi-like history browser.

## What To Borrow Now

For the current transcript screen, the best near-term model is:

1. **Keep message-level selection in the core model**
   - do not move the domain model to line-level scrolling

2. **Introduce an explicit visual scroll offset**
   - separate from selected message index
   - likely stored in transcript view state in the shared core

3. **Compute wrapped line layout for only the needed window**
   - selected message
   - a bounded number of messages above and below
   - enough overflow to fill the viewport

4. **Track message-to-line mapping during render preparation**
   - `message_start_lines`
   - possibly `message_line_ranges`

5. **Render from a prebuilt flat line buffer**
   - not directly from nested `MessagePreview` loops into a giant `List`

6. **Use selection by stable message/entry ID where possible**
   - current message index is acceptable short-term
   - ID-based selection becomes more important once transcript structure gets richer

## What To Borrow Later

For the future Pi-like history browser:

1. build a tree of `conversation_entry`
2. flatten it into visible rows
3. preserve selection by entry ID
4. support folding/filtering on the flattened representation
5. render only a visible window of rows

This should be treated as a separate rendering mode from the current transcript reader, even if both share some visual components.

## What Not To Copy

From `recall`:

- do not copy its lossy transcript model
- do not reparse raw provider files in the render path
- do not let preview rendering become the primary transcript abstraction

From Pi:

- do not copy the DOM/export implementation
- do not copy UI-local state patterns that belong in the Crux core
- do not assume manual windowing around selection is sufficient forever if transcript sizes grow substantially

## Recommended Next Refactor

The next transcript rendering refactor should be:

1. **Introduce transcript viewport state**
   - selected message index stays
   - add visual scroll offset or equivalent derived cursor anchor

2. **Replace full-transcript render with windowed render preparation**
   - compute only enough wrapped lines to fill the viewport
   - include some context above/below the selected entry

3. **Represent the render input as flat visible lines**
   - each line carries:
     - owning message index or ID
     - style information
     - whether it belongs to the selected entry

4. **Keep selection styling explicit**
   - selected transcript entry must remain visually obvious

5. **Re-profile with `--profile`**
   - confirm render time drops materially on repeated transcript scroll interactions

## Suggested Division Of Responsibilities

Shared core:

- selected message / selected entry
- transcript viewport state
- future selection-by-ID semantics

CLI renderer:

- wrapping
- line layout
- visible slice construction
- final Ratatui painting

This preserves the Crux split while keeping expensive layout logic out of the core domain.

## Practical Rule Of Thumb

- **Linear transcript reader:** use `recall` as the closer inspiration
- **Pi-like history/tree browser:** use Pi as the closer inspiration

Trying to force one rendering model to serve both well will likely create unnecessary complexity.
