# cc-sessions Analysis Report

## Overview
- **Repository:** `cc-sessions` (Local path: `~/playground/ai-stuff/harness-utils/session-managers/cc-sessions`)
- **Category / Family:** Session Manager / Transcript Indexer / Terminal UI (TUI) Utility
- **Purpose:** A CLI tool to search, preview, resume, and fork Claude Code sessions across various projects and machines. It solves the limitation of the native `/resume` command, which only exposes sessions bound to the current project directory and local machine.
- **Provider Coverage:** Claude Code (`.jsonl` files).

## Architecture

```text
┌─────────────────────────────────────────────────────────┐
│                    cc-sessions CLI                      │
├─────────────────────────────────────────────────────────┤
│ ┌────────────────┐   ┌────────────┐   ┌───────────────┐ │
│ │  Remote Sync   │   │  Skim TUI  │◄──┤  Interactive  │ │
│ │  (SSH/Rsync)   │   │(Left/Right)│   │  State Mach.  │ │
│ └───────┬────────┘   └──────▲─────┘   └───────▲───────┘ │
│         │                   │                 │         │
│ ┌───────▼───────────────────┴─────────────────▼───────┐ │
│ │         Session Domain Model (src/session.rs)       │ │
│ └───────▲─────────────────────────────────────────────┘ │
│         │                                               │
│ ┌───────┴────────┐   ┌────────────────────────────────┐ │
│ │  Claude Code   ├───►    Message Classification      │ │
│ │ Ingestion (I/O)│   │ (isSidechain, turn count, etc) │ │
│ └───────▲────────┘   └────────────────────────────────┘ │
│         │                                               │
└─────────┼───────────────────────────────────────────────┘
          │ (Parallel Scan via Rayon)
  ┌───────┴────────┐
  │ ~/.claude/     │
  │ projects/*.jsonl
  └────────────────┘
```

## Big-Picture Architecture & Strategies

### Ingestion, Normalization, and Indexing
- **Single-Pass Streaming Extraction:** The core ingestion logic (`src/claude_code.rs`) utilizes `Rayon` for parallelized file scanning. It performs a single-pass streaming read over each `.jsonl` session file using `BufReader::new(file).lines()`.
- **In-Memory Search Indexing:** It collects chunks of text (from both `user` and `assistant` messages) during the scan, concatenating and lowercasing them into a `search_text_lower` field on the `Session` struct. This acts as an in-memory index for rapid `Ctrl+S` grep-like searches within the UI, bypassing the need to re-read files from disk during active use.
- **Noise Filtration (Normalization):** Synthetic outputs such as `isMeta` (context attachments), `isCompactSummary`, `isSidechain` (subagents), and Swarm teammate interactions are explicitly filtered out during parsing (`src/message_classification.rs`) to keep turn counts and search indexes relevant to the primary human-driven conversation.

### Remote Update Mechanism
- **Multi-Machine Sync:** The `src/remote.rs` module enables indexing sessions from remote servers using a configuration (`~/.config/cc-sessions/remotes.toml`).
- **Update Flow:** A sync operation (invoked via `--sync` or auto-synced if stale) executes SSH/rsync to pull remote metadata into the local viewer. Failures gracefully fallback, maintaining a high-availability view of the sessions that successfully loaded.

### Transcript Structure Model
- **Event-Driven JSONL:** It models transcripts fundamentally as a sequential event stream of `serde_json::Value` lines.
- **Hierarchical Trees:** It parses `forkedFrom.sessionId` fields to link child sessions to their parents. This metadata is mapped into a `HashMap` of children, allowing the application to construct and render a nested tree structure (e.g., Parent -> Fork 1, Fork 2) dynamically.
- **Extracted Fields:** The model captures `project_path`, `first_prompt`, `turn_count`, `summary`, `custom_title` (via `/rename`), and `tag` (via `/tag`). It relies on filesystem metadata (ctime/mtime) for temporal sorting rather than parsing internal API timestamps.

## UX Shape
- **Fuzzy Finder Foundation:** Built atop `skim`, the interface opts for a robust dual-pane terminal layout.
- **List and Tree View (Left Pane):** Displays tabular columns (`CRE`, `MOD`, `MSG`, `SOURCE`, `PROJECT`, `SUMMARY`). It utilizes simple string prefixing (`▷` and `▶`) to indicate tree depth and fork existence. Arrow keys (`Right`/`Left`) navigate down into and up out of session forks visually.
- **Preview View (Right Pane):** Renders a live-formatted transcript with ANSI coloring (Cyan for User `U:`, Yellow for Assistant `A:`). It strips system boilerplate and handles text wrapping automatically natively in Rust.
- **Full-Text Filter Prompt:** Hitting `Ctrl+S` swaps the fuzzy list prompt for a precise substring query prompt against the pre-computed `search_text_lower` values, instantaneously narrowing the list to matching sessions and highlighting content in the preview pane.

## Strengths
- **Performance:** Ingestion is incredibly fast due to parallelized, single-pass streaming reads without deserializing the entire JSON structure into memory at once.
- **Separation of Concerns:** Excellent architectural decoupling between the CLI orchestration (`main.rs`), pure UI State (`InteractiveState`), core Domain (`Session`), and un-opinionated I/O (`claude_code.rs`).
- **Resilience:** Does not break if `jq` or `jaq` are missing, as preview rendering was internalized in Rust. Graceful degradation when remote syncs fail.

## Weaknesses
- **Tight Coupling to Schema:** Inherently brittle if Anthropic silently modifies the undocumented internal Claude Code `.jsonl` schema (e.g., renaming `isSidechain` or `forkedFrom`).
- **Scalability of Index:** Storing the full concatenated lowercased text of every session in memory (`search_text_lower`) could become a memory constraint for users with thousands of massive, uncompacted sessions.
- **Primitive Search:** Relies entirely on basic substring matching. It lacks semantic search, embeddings, or complex boolean queries.

## Key Code Snippets

### 1. Single-Pass Streaming Ingestion
```rust
// From src/claude_code.rs: Efficiently reads JSONL lines, extracts metadata and populates the search index.
for line in BufReader::new(file).lines().map_while(Result::ok) {
    let entry: serde_json::Value = match serde_json::from_str(&line) {
        Ok(v) => v,
        Err(_) => continue,
    };

    // Filter out internal/noisy subagent sessions early
    if entry.get("isSidechain").and_then(|v| v.as_bool()) == Some(true) {
        scan.skip = true;
        return scan;
    }

    // Extract Fork Lineage
    if scan.forked_from.is_none()
        && let Some(parent_id) = entry.get("forkedFrom").and_then(|f| f.get("sessionId")).and_then(|v| v.as_str())
    {
        scan.forked_from = Some(parent_id.to_string());
    }

    // Build the in-memory search index
    if matches!(entry_type, Some("user") | Some("assistant"))
        && let Some(text) = extract_message_text_for_search(&entry)
    {
        search_chunks.push(text);
    }
}
scan.search_text_lower = search_chunks.join("\n").to_lowercase();
```

### 2. State Machine for TUI Interactions
```rust
// From src/interactive_state.rs: Pure state management for the TUI, isolated from skim context.
pub fn apply(&mut self, action: Action) -> Effect {
    match action {
        Action::Esc => {
            if self.search_results.is_some() {
                self.search_results = None;
                self.search_pattern = None;
                return Effect::Continue;
            }
            if !self.focus_stack.is_empty() {
                self.focus_stack.clear();
                return Effect::Continue;
            }
            Effect::Exit
        }
        // ... (handles drilling into forks with Right arrow, executing searches with CtrlS)
    }
}
```

### 3. Integrated Custom Preview Pane
```rust
// From src/main.rs: Generates ANSI-colored preview dynamically for skim
match entry_type {
    Some("user") => {
        if let Some(text) = extract_message_text(&entry) && !is_system_content(&text) {
            let first_line = text.lines().next().unwrap_or(&text);
            let truncated = truncate_str(first_line, 120);
            output.push_str(&format!("{}U: {}{}\n", colors::CYAN, truncated, colors::RESET));
            line_count += 1;
        }
    }
    // ...
}
```