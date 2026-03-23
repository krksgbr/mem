# Research Synthesis: Transcript Indexer & Browser Survey

**Date:** 2026-03-23
**Goal:** Extract architectural patterns from a curated set of local transcript/session managers to inform the design of a terminal-first, local-first, fast-starting transcript browser.

## 1. Taxonomy of Architectural Families

Based on the 9 surveyed repositories, the tools fall into three distinct architectural families:

1. **Ephemeral Raw Scanners (`cc-sessions`, `ccs`, `claude-history`, `transcript-browser`)**
   - *Pattern:* No persistent database. They read directly from provider log files (e.g., `~/.claude/projects/*.jsonl`) into memory on every startup.
   - *Strengths:* Zero state, no migrations, structurally simple.
   - *Weaknesses:* Startup latency degrades linearly as history grows. They typically rely on heavy parallelization (Rust `Rayon`, Go worker pools) to mask this, but the scalability ceiling is low.
2. **Persistent Local Indexers (`ccrider`, `coding_agent_session_search`, `memex`, `recall`)**
   - *Pattern:* They maintain a local database (SQLite FTS5, Tantivy, USearch) populated by a background sync or daemon process.
   - *Strengths:* Sub-100ms startup and search latency regardless of history size. Enables complex querying (e.g., lexical + semantic hybrid search).
   - *Weaknesses:* Requires managing state, sync lifecycle, and disk space for the index.
3. **Stateless Web Streamers (`claude-run`)**
   - *Pattern:* Lightweight backend server watching logs and pushing Server-Sent Events (SSE) to a Web UI.
   - *Strengths:* Real-time updates without heavy state.
   - *Weaknesses:* Not terminal-first.

## 2. Comparison Matrix

| Repo | Ingestion | Storage / Index | Update Strategy | Provider Coverage | Normalization | Primary UX |
|------|-----------|-----------------|-----------------|-------------------|---------------|------------|
| `cc-sessions` | Streaming (Rayon) | In-Memory (Strings) | Restart/Remote Sync | Claude Code | Keep Tree, Drop Noise | TUI (List+Tree) |
| `ccrider` | Go Workers | SQLite (FTS5) | Incremental Sync | Claude, Codex | Lossless Structure | TUI (Browse+Search) |
| `ccs` | Go Workers | In-Memory | Restart | Claude Code | Lossy (Text only) | TUI (Search-first) |
| `claude-history` | Streaming (Rayon) | In-Memory | Restart | Claude Code | Lossless Structure | TUI (Fuzzy Browse) |
| `claude-run` | On-demand stream | Memory Cache | `chokidar` (SSE) | Claude Code | Lossless Structure | Web UI |
| `coding_agent...` | Background Sync | Tantivy + SQLite | `notify` watch | ALL Agents | Generic + JSON | TUI (3-Pane) |
| `memex` | Mmap + Rayon | Tantivy + USearch | Incremental (mtime) | Claude, Codex, OpenCode | Flat Record (Keeps Tools) | TUI + CLI API |
| `recall` | Background Sync | Tantivy | `mtime` poll/sync | Claude, Codex, etc | Lossy (Text only) | TUI (Search-first) |
| `transcript-browser`| Sync Blocking | In-Memory | Restart | Claude Code | Lossy (Text only) | TUI (Drill-down) |

## 3. Analysis by Key Dimensions

### Ingestion & Normalization
- **Lossy vs. Lossless:** Projects like `ccs`, `recall`, and `transcript-browser` flatten the complex JSON objects (like Anthropic's tool uses and thinking blocks) into raw text. This makes them fast for pure search but terrible for *rich browsing*. Conversely, `ccrider`, `claude-history`, `claude-run`, and `memex` retain schema fidelity (or at least map tool execution distinctly), allowing users to toggle visibility of tools and thinking blocks.
- **Hierarchy:** Advanced tools (`cc-sessions`, `claude-history`) parse `forkedFrom` IDs to build proper conversation trees. Simpler tools treat all files as flat sessions.
- **Speed:** `memex` pushes the limit of disk I/O by utilizing memory-mapped files (`mmap`) combined with `rayon` for parallel, zero-copy JSON parsing during its ingestion phase.

### Storage & Indexing (Raw vs Persistent)
- The raw-file scanning approach (used by 4 out of 9 tools) clearly violates our "startup responsiveness matters" priority as log directories grow into the gigabytes.
- For persistent indexing, tools chose either **SQLite + FTS5** (`ccrider`) for structured relational data + search, or **Tantivy** (`recall`, `memex`, `coding_agent_session_search`) for pure full-text speed. `memex` and `coding_agent_session_search` also add vector DBs for semantic search.

### Update Strategies
- The lack of file watching in `claude-history`, `ccs`, and `transcript-browser` makes them purely post-mortem tools. 
- Successful live-watching approaches either use OS-level events (`chokidar` in `claude-run`, `notify` in `coding_agent_session_search`) or an incremental byte-offset/`mtime` syncing strategy (`ccrider`, `recall`, `memex`) to only parse appended bytes.

### Browse UX vs Search UX
- **Search-first:** `recall`, `memex`, and `ccs` drop you immediately into an empty prompt.
- **Browse-first:** `cc-sessions` and `claude-history` use list/tree paradigms (like `fzf` or `skim`), which aligns better with our "rich browsing matters" priority.

## 4. Shortlists & Recommendations

### Best Inspirations (Compatible with our priorities)
- **`ccrider`**: Best overall balance. The SQLite FTS5 backend provides excellent speed while maintaining structured data (Lossless) needed for rich browsing. Its incremental byte-offset sync is highly efficient.
- **`claude-history`**: Best UX inspiration for *rich browsing* (folding/unfolding tool blocks and thinking steps natively in the terminal).
- **`memex` & `recall`**: The dual TUI/CLI architecture is a smart pattern that serves both human engineers and headless agents. `memex`'s use of `mmap` for ultra-fast incremental parsing is a stellar technical inspiration.

### Probably Not a Fit (Overbuilt or incompatible)
- **`coding_agent_session_search` & `memex` (Vector capabilities)**: Hybrid search and local embeddings are incredible for deep search but are resource-intensive and overbuilt for our immediate goal of a fast, structured transcript browser.
- **`claude-run`**: Web UI, violating our terminal-first mandate.
- **`transcript-browser`**: Strict Elm-architecture (Crux) led to a fully synchronous blocking startup, violating our latency priority.
- **Ephemeral Scanners (`ccs`, `cc-sessions`)**: Will not scale long-term for power users.

## 5. 3-5 Architecture Directions to Consider Next

Based on this design space exploration, we should explore these directions in our next phase:

1. **Persistent Local Index (SQLite or Tantivy):** Abandon ephemeral scanning. Adopt a local index (like `ccrider`'s SQLite or `memex`'s Tantivy) that maintains tool metadata for rich browsing while guaranteeing fast startup. 
2. **Incremental Mmap/Byte-offset Watcher:** Decouple ingestion from the UI. A lightweight background watcher (or a fast sync on startup like `memex`) that only reads the *appended bytes* of active `.jsonl` files to support near real-time transcript watching.
3. **Lossless Normalization Schema:** Design an internal data model that unifies Claude Code and Codex, but *preserves* complex blocks (Tools, Thinking, System prompts). Do not flatten to raw text, as rich browsing is a core priority.
4. **Dual-Mode TUI (Browse + Search):** Combine the visual tree-navigation of `cc-sessions` with the instantaneous fuzzy-search of `recall`/`memex`. The UI must support collapsing/expanding tool executions.
5. **Decoupled Architecture:** Build the core logic in a pure library separate from the terminal renderer (Ratatui/Bubbletea) to ensure high testability, but avoid purely synchronous blocking state machines.