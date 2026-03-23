# Research Report: coding-agent-session-search (cass)
**Date:** 2026-03-23
**Researcher:** Gemini CLI

## 1. Overview
`coding-agent-session-search` (executable: `cass`) is a high-performance, unified search engine and TUI (Terminal User Interface) designed to aggregate and index conversation histories from a wide variety of AI coding agents. It transforms fragmented local agent logs into a queryable knowledge base with sub-60ms latency.

- **Repo Name:** `coding_agent_session_search`
- **Category/Family:** Transcript Indexer / Agent History Manager / Multi-Agent Aggregator
- **Purpose:** Provide a centralized, local-first, privacy-respecting interface to search across all AI coding agent sessions (Lexical, Semantic, and Hybrid search).

## 2. Architecture Diagram (ASCII)

```text
       ┌─────────────────────────────────────────────────────────┐
       │                  USER INTERFACES                        │
       │  ┌───────────────┐  ┌───────────────┐  ┌─────────────┐  │
       │  │  FrankenTUI   │  │   Robot CLI   │  │ HTML Export │  │
       │  └──────┬────────┘  └───────┬───────┘  └──────┬──────┘  │
       └─────────┼───────────────────┼─────────────────┼─────────┘
                 │                   │                 │
                 ▼                   ▼                 ▼
       ┌─────────────────────────────────────────────────────────┐
       │               SEARCH & ANALYTICS ORCHESTRATOR           │
       │  ┌───────────────────┐        ┌──────────────────────┐  │
       │  │   Hybrid Search   │        │ Analytics Dashboard  │  │
       │  │ (RRF: Lex+Sem)    │        │ (7 specialized views)│  │
       │  └────────┬──────────┘        └───────────┬──────────┘  │
       └───────────┼───────────────────────────────┼─────────────┘
                   ▼                               ▼
       ┌─────────────────────────────────────────────────────────┐
       │               CORE ENGINE & DATA LAYERS                 │
       │  ┌──────────────────┐  ┌─────────────────────────────┐  │
       │  │  Tantivy Index   │  │   SQLite (Metadata/Store)   │  │
       │  │ (Full-Text/Ngram)│  │ (Conversations/Messages)    │  │
       │  └────────┬─────────┘  └──────────────┬──────────────┘  │
       │           │                           │                 │
       │  ┌────────▼─────────┐  ┌──────────────▼──────────────┐  │
       │  │ Vector Index     │  │  Background Indexer         │  │
       │  │ (FSVI / HNSW)    │  │ (notify + sync + provenance)│  │
       │  └────────┬─────────┘  └──────────────┬──────────────┘  │
       └───────────┼───────────────────────────┼─────────────────┘
                   │                           │
                   ▼                           ▼
       ┌─────────────────────────────────────────────────────────┐
       │                INGESTION & NORMALIZATION                │
       │  ┌───────────────────────────────────────────────────┐  │
       │  │      franken_agent_detection (Connectors)         │  │
       │  │ (Claude, Codex, Cursor, Aider, Cline, Gemini, etc)│  │
       │  └───────────────────────────────────────────────────┘  │
       └─────────────────────────────────────────────────────────┘
```

## 3. Key Components

### Provider Coverage
`cass` supports a broad spectrum of AI coding agents:
- **Major Agents:** Claude Code, Codex, Cursor, ChatGPT, Aider, Cline.
- **Others:** Gemini CLI, OpenCode, Amp, Pi-Agent, Factory (Droid), Kimi, Qwen.

### Ingestion & Normalization Strategy
- **Extraction:** Ingestion logic is delegated to the `franken_agent_detection` crate, which scans local directories for known agent signatures.
- **Normalization:** Disparate formats (JSONL, SQLite, Markdown) are mapped to a `NormalizedConversation` domain model.
- **Provenance:** Tracks the source of truth (local vs. remote) and host labels for multi-machine setups.

### Storage & Indexing Strategy
- **Metadata:** SQLite stores structured relationships (Agents, Workspaces, Conversations, Messages, Snippets).
- **Lexical Index:** Tantivy (via `frankensearch`) provides BM25 full-text search with **Edge N-Grams** for O(1) "search-as-you-type" prefix matching.
- **Semantic Index:** Optional vector indexing using FastEmbed (MiniLM) or a deterministic hash-based fallback.
- **Hybrid Search:** Uses **Reciprocal Rank Fusion (RRF)** to combine lexical and semantic results.

### Update Mechanism
- **Real-time:** Uses `notify` to watch file system changes and trigger incremental indexing.
- **Stability:** Background worker with debounced reloads ensures the UI remains responsive during heavy ingestion.
- **Stale Detection:** Monitors ingest success rates and can trigger automatic full rebuilds if the index drifts.

### UX Shape
- **TUI-First:** A sophisticated 3-pane layout built with FrankenTUI, featuring adaptive frames and animations.
- **Robot Mode:** A machine-readable CLI interface (`--robot`, `--json`) for other AI agents to query history.
- **Analytics:** Integrated dashboard with heatmaps and breakdown charts.

## 4. Strengths & Weaknesses

### Strengths
- **Performance:** Sub-60ms latency; highly optimized Rust implementation.
- **Versatility:** Aggregates almost every major coding agent into one timeline.
- **Local-First:** Privacy-preserving; no network dependency for core features.
- **Hybrid Search:** Combines precision of lexical matching with the recall of semantic search.
- **Rich Export:** Secure (AES-encrypted) HTML exports for sharing conversations.

### Weaknesses
- **Complexity:** Heavy reliance on the "Franken-stack" (frankensearch, frankentui, frankensqlite) may increase maintenance surface.
- **Storage Overhead:** Edge N-Grams and vector indexes trade disk space for speed.
- **Manual Setup:** Semantic search requires manual model file placement to avoid hidden downloads.

## 5. Key Code Snippets

### Domain Model (Normalized Message)
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: Option<i64>,
    pub idx: i64,
    pub role: MessageRole,
    pub author: Option<String>,
    pub created_at: Option<i64>,
    pub content: String,
    pub extra_json: serde_json::Value,
    pub snippets: Vec<Snippet>,
}
```

### Hybrid Fusion (Conceptual RRF)
The project uses `Reciprocal Rank Fusion` to merge Lexical and Semantic ranks:
`Score = Σ(1 / (K + rank))` where `K=60`.

### Database Schema (SQLite)
```sql
CREATE TABLE IF NOT EXISTS conversations (
    id INTEGER PRIMARY KEY,
    agent_id INTEGER NOT NULL REFERENCES agents(id),
    workspace_id INTEGER REFERENCES workspaces(id),
    external_id TEXT,
    title TEXT,
    source_path TEXT NOT NULL,
    started_at INTEGER,
    ended_at INTEGER,
    approx_tokens INTEGER,
    metadata_json TEXT,
    UNIQUE(agent_id, external_id)
);
```
