# Indexing Architecture

## Context

`transcript-browser` started as a spike that browsed raw provider transcripts directly from disk.
That was enough to validate the interface direction, but it is not enough for the product we now want to build.

The product has two first-class jobs:

- a human-facing workspace manager for browsing, garbage collection, and history repair
- an agent-facing memory interconnect for cross-provider search and transcript retrieval

The current raw-scan design is the wrong foundation for that product because it couples startup cost to transcript corpus size and makes every search or browse feature fight raw provider storage directly.

## Problem Statement

The current design has three proven shortcomings:

1. Startup is too slow because transcript discovery and parsing happen on the critical path.
2. The app has no durable shared query substrate for both human and agent workflows.
3. Search, retrieval, and viewing are still too close to provider-specific raw storage.

The important observation is this:

We do need synchronized derived state.
What we do not need is to pretend that derived state is the source of truth.

Raw transcript files remain canonical.
The indexed store is a rebuildable local projection used to make the product fast and queryable.

## Goals

### Must

- Start quickly from indexed local state rather than reparsing the full corpus on every launch.
- Keep raw transcript files as the canonical source of truth.
- Support Claude Code and Codex from the start.
- Store enough normalized structure for:
  - conversation-level browsing
  - entry-level search
  - tool-aware transcript reads
  - tree-based history navigation
  - provider/source provenance
- Power both product surfaces from the same local indexed store:
  - human TUI
  - agent-facing CLI/JSON
- Sync incrementally and continue syncing in the background when needed.
- Remain local-first and pure Rust.

### Should

- Be easy to extend to additional providers.
- Support live transcript updates while the app is open.
- Make rebuild-on-schema-change straightforward.
- Keep provider-specific logic confined to import/parsing.

### Not Now

- Live pipes between active agents
- Remote or distributed sync
- Mandatory semantic/vector search
- A plugin architecture for third-party providers
- Durable schema migrations for indexed data

## Requirements

### Product Requirements

- Humans must be able to browse workspaces and conversations quickly, even with a large local history corpus.
- Humans must be able to navigate transcript history in a browser broadly similar to Pi's `/tree` UI as a first pass, then refine from there with transcript-browser-specific customizations.
- Humans must be able to identify and delete low-value sessions from disk.
- Humans must be able to detect orphaned histories and repair project associations.
- Agents must be able to search globally across providers and projects.
- Agents must be able to fetch a relevant transcript window without navigating raw provider files directly.
- Search must be useful for both humans and agents, not just a low-level grep wrapper.

### Technical Requirements

- Raw files are canonical; the index is derived and rebuildable.
- The app must be usable from the last indexed state even while sync is catching up.
- Parser and sync failures must preserve source-file context.
- Search should operate over normalized indexed data.
- Read views should come from indexed normalized data, not reparse raw files on every request.
- Query-time filtering is preferred over aggressive data loss at ingest time.

## Core Decision

Use a local SQLite database as the primary queryable materialization of transcript history.

Raw transcript files remain canonical input.
SQLite is a rebuildable derived store used for startup, browsing, search, transcript reads, tombstones, and sync state.

This is intentionally a stronger decision than “keep a lightweight metadata index.”
We considered that lighter approach, but it still required a real sync mechanism while leaving too much product value dependent on raw-file hydration.

Given that:

- schema rebuilds are acceptable
- the product needs global search and transcript retrieval
- the app should remain useful while syncing in the background

it is simpler to maintain one broad derived store than to invent a thinner custom index plus a separate raw-read substrate.

## Why SQLite

SQLite fits the current problem better than a search-engine-first or vector-first design.

It gives us:

- fast startup from local indexed state
- relational modeling for workspaces, conversations, conversation entries, blocks, and sources
- transactional incremental sync
- inspectable local state
- a straightforward lexical-search path via FTS
- one boring embedded dependency instead of a multi-engine system

It also fits the product shape better than a pure search index because the product is not only a search tool.
It is also a workspace/history manager that needs provenance, tombstones, file-state tracking, and repair workflows.

## UI Target

The first-pass transcript browser should aim for a navigation experience similar in spirit to Pi's history/tree browser:

- tree-aware transcript navigation
- collapsible branches and event groups
- filtering by entry kinds
- transcript reading that preserves non-message history events such as summaries and compactions

This is a UX target, not a commitment to Pi's exact implementation or terminology.
The key implication for storage is that the model must support a navigable history spine of heterogeneous entries, not just a flat list of messages.

## Search Engine Scope

The initial search implementation should stay inside SQLite via FTS.

We considered the split-store pattern used in `rhizome`, where:

- SQLite stores structured application data
- a second embedded search engine stores search documents
- the application stitches ranked search hits back to SQLite rows

That pattern is valid, but it adds a second derived store and a second sync surface.

For `transcript-browser`, that is premature in the first architecture.
We already need one derived store for normalized transcript state and sync bookkeeping.
Adding a separate search engine should be deferred until SQLite FTS proves insufficient on one of these axes:

- ranking quality
- query latency
- typo tolerance / retrieval quality

So the plan is:

- start with SQLite as both the normalized store and the lexical search substrate
- keep the query layer abstract enough that a secondary search engine can be added later
- only adopt a `milli`-style split store if measurement justifies the added complexity

## Architectural Center

The architectural center of the system is:

> Raw transcript files are canonical. SQLite stores a broad normalized projection that is rebuildable and optimized for product queries.

That implies:

- startup reads from SQLite, not from provider directories
- search reads from SQLite
- transcript views read from SQLite
- provider parsers feed the importer, not the UI
- rebuild is the escape hatch for schema evolution

## System Boundaries

The system should be split into four subsystems.

### 1. Import Layer

Responsible for:

- discovering provider source files
- parsing provider-specific transcript formats
- normalizing provider events into shared entities
- computing sync deltas
- writing updated rows into SQLite

This is the only layer that should understand Claude- and Codex-specific on-disk schemas.

### 2. Indexed Store

Responsible for:

- storing normalized transcript entities
- storing source-file sync state
- storing tombstones and source status
- supporting browse, read, and search queries

This is the main read substrate for the product.

### 3. Query Layer

Responsible for:

- exposing product-oriented read/query APIs over SQLite
- grouping entry-level hits into conversation-level results
- applying query-time filtering and ranking
- shaping transcript windows and pages for output

This layer should not parse raw provider files.

### 4. Product Surfaces

Responsible for:

- TUI browse/manage UX
- headless CLI / JSON output
- later MCP integration if needed

These surfaces should depend on the query layer, not the import layer.

## Sync Model

### Source of Truth

- Raw transcript files are the only canonical content source.
- SQLite is a rebuildable projection.
- If the indexed schema changes materially, wiping and rebuilding the index is acceptable.

### Startup Behavior

- On startup, the app opens the existing SQLite index immediately.
- It then begins incremental sync automatically.
- If there is a backlog, the app remains usable from the last indexed state while sync continues in the background.

### Background Sync

- Sync should process only changed files where possible.
- New or updated transcripts should appear live in the TUI.
- The UI should expose sync status clearly, but should not jump around unexpectedly.

### Deletions and Moves

- Deleted or moved source files should not simply vanish from history.
- The indexed store should keep tombstones or source-status records so that:
  - broken histories can be diagnosed
  - migration workflows can repair them
  - normal search results can exclude dead content by default

## Normalized Data Model

The indexed store should favor one generic history model with provider-specific metadata attached where needed.

### First-Class Entities

- `source_file`
  - provider
  - path
  - sync status
  - mtime / size / fingerprint state
- `workspace`
  - normalized workspace identity
  - display path/name
- `conversation`
  - stable conversation identity
  - provider
  - workspace association
  - title / preview
  - created / updated timestamps
- `conversation_entry`
  - stable external entry identity
  - conversation association
  - `kind`
  - timestamp / ordering
  - `parent_entry_id` for history/tree structure
  - `associated_entry_id` for grouping related events without changing the tree
- `block`
  - optional typed payload attached to entries that need structured content
  - examples: text, image, json payloads, provider-specific structured fields

### Core Entry Kinds

The core entry kinds should describe concepts that are common or plausibly common across harnesses:

- `user_message`
- `assistant_message`
- `tool_call`
- `tool_result`
- `thinking`
- `summary`
- `compaction`
- `label`
- `metadata_change`

These kinds are intentionally generic.
We should not bake Pi-specific names directly into the core schema unless they prove broadly useful later.

### Why `conversation_entry` Is The Spine

The browser we want is not just a viewer over messages.
It needs to navigate a transcript history made up of heterogeneous events:

- user and assistant messages
- tool calls and tool results
- thinking/reasoning traces
- inserted summaries
- compaction boundaries
- labels and metadata changes

That makes `conversation_entry` the correct persisted spine.

### Relationship Semantics

`parent_entry_id` and `associated_entry_id` must remain distinct.

- `parent_entry_id`
  expresses transcript/history tree structure
- `associated_entry_id`
  expresses semantic grouping between related events

Examples:

- a `thinking` entry may be associated with the assistant message it belongs to
- a `tool_call` may be associated with the assistant message that emitted it
- a `tool_result` may be associated with the `tool_call` it answers

This gives the UI enough structure to add grouped rendering later without introducing a first-class `turn` table in v1.

### Provider-Specific Provenance

Provider-specific details should be stored as metadata on the relevant entities rather than promoted to the core taxonomy too early.

This is especially important for concepts like Codex rollout/source chunk groupings.
Those should be preserved, but not centered in the whole schema unless multiple providers prove the same concept is foundational.

### Provider-Specific Metadata

The generic model should be the center.
Provider-specific details should be stored as additional metadata fields or JSON blobs where needed, rather than driving the top-level schema.

## Concrete SQLite Schema

This section defines the first-pass persisted schema closely enough to start building.

The intent is:

- keep the schema boring
- preserve core transcript structure explicitly
- preserve provider-specific details without overfitting the top-level model
- make rebuilds cheap enough that schema evolution stays practical

### Conventions

- Every primary product entity gets a stable external string ID.
- SQLite may also use integer rowids internally for joins and FTS, but those are not part of the public interface.
- Timestamps should be stored as integer Unix milliseconds where available.
- Provider-specific metadata should default to JSON text columns, not one-off side tables, unless a field becomes heavily queried.

### `source_file`

Tracks canonical raw transcript sources and sync bookkeeping.

Suggested columns:

- `id TEXT PRIMARY KEY`
- `provider TEXT NOT NULL`
- `path TEXT NOT NULL UNIQUE`
- `status TEXT NOT NULL`
  - expected values: `active`, `missing`, `moved`, `parse_error`, `ignored`
- `workspace_hint TEXT`
- `file_size_bytes INTEGER`
- `mtime_ms INTEGER`
- `content_fingerprint TEXT`
- `last_indexed_at_ms INTEGER`
- `last_seen_at_ms INTEGER`
- `parse_error TEXT`
- `provider_metadata_json TEXT`

Purpose:

- incremental sync decisions
- tombstone/source diagnostics
- repair and migration workflows

### `workspace`

Represents the normalized project/workspace concept used by the browser.

Suggested columns:

- `id TEXT PRIMARY KEY`
- `canonical_path TEXT`
- `display_name TEXT NOT NULL`
- `provider_scope TEXT`
- `status TEXT NOT NULL DEFAULT 'active'`
- `created_at_ms INTEGER`
- `updated_at_ms INTEGER`
- `metadata_json TEXT`

Notes:

- `canonical_path` may be null for providers that do not have a stable local project path
- multiple providers may map into one workspace if that proves useful, but the schema should not force that merge too early

### `conversation`

Represents a logical transcript/session/thread.

Suggested columns:

- `id TEXT PRIMARY KEY`
- `workspace_id TEXT REFERENCES workspace(id)`
- `provider TEXT NOT NULL`
- `provider_conversation_id TEXT`
- `title TEXT`
- `preview_text TEXT`
- `status TEXT NOT NULL DEFAULT 'active'`
- `created_at_ms INTEGER`
- `updated_at_ms INTEGER`
- `last_source_event_at_ms INTEGER`
- `metadata_json TEXT`

Recommended indexes:

- `(workspace_id, updated_at_ms DESC)`
- `(provider, updated_at_ms DESC)`
- `(provider, provider_conversation_id)`

### `conversation_source_file`

Join table from logical conversations to raw source files.

Suggested columns:

- `conversation_id TEXT NOT NULL REFERENCES conversation(id)`
- `source_file_id TEXT NOT NULL REFERENCES source_file(id)`
- `role TEXT`
  - examples: `primary`, `segment`, `auxiliary`
- `ordinal INTEGER`
- `metadata_json TEXT`
- `PRIMARY KEY (conversation_id, source_file_id)`

Purpose:

- preserve multi-file provenance, including Codex rollout/source chunk structure, without centering the whole schema on `segment`

### `conversation_entry`

This is the core history spine.

Suggested columns:

- `id TEXT PRIMARY KEY`
- `conversation_id TEXT NOT NULL REFERENCES conversation(id)`
- `kind TEXT NOT NULL`
- `parent_entry_id TEXT REFERENCES conversation_entry(id)`
- `associated_entry_id TEXT REFERENCES conversation_entry(id)`
- `source_file_id TEXT REFERENCES source_file(id)`
- `provider_entry_id TEXT`
- `ordinal INTEGER NOT NULL`
- `timestamp_ms INTEGER`
- `is_searchable INTEGER NOT NULL DEFAULT 1`
- `search_text TEXT`
- `summary_text TEXT`
- `metadata_json TEXT`

Recommended indexes:

- `(conversation_id, ordinal)`
- `(conversation_id, timestamp_ms)`
- `(parent_entry_id)`
- `(associated_entry_id)`
- `(provider_entry_id)`

Notes:

- `ordinal` is the primary stable ordering within a conversation
- `timestamp_ms` is supplemental because provider timestamps may be missing or noisy
- `search_text` is the extracted lexical surface used for default search
- `summary_text` is an optional concise display/search aid for kinds that are not naturally textual
- `is_searchable = 0` should be used for kinds like `thinking` by default

### `conversation_entry.kind`

Initial allowed values:

- `user_message`
- `assistant_message`
- `tool_call`
- `tool_result`
- `thinking`
- `summary`
- `compaction`
- `label`
- `metadata_change`

These should be enforced in application code first.
A SQL `CHECK` constraint can be added once the set proves stable enough.

### `entry_block`

Optional structured payload attached to entries that need richer rendering than `search_text` / `summary_text`.

Suggested columns:

- `id TEXT PRIMARY KEY`
- `entry_id TEXT NOT NULL REFERENCES conversation_entry(id)`
- `ordinal INTEGER NOT NULL`
- `kind TEXT NOT NULL`
- `text_value TEXT`
- `json_value TEXT`
- `mime_type TEXT`
- `metadata_json TEXT`

Initial block kinds:

- `text`
- `image`
- `json`
- `structured`

Usage:

- `assistant_message` may have one or more text/image blocks
- `tool_call` can store arguments in `json_value`
- `tool_result` can store text blocks plus richer structured payloads
- `thinking` can store text as a block while remaining a top-level entry kind

This is intentionally less typed than Pi's in-memory message unions.
The schema should preserve structure without forcing a large number of narrow tables too early.

### `entry_label`

Explicit labels/bookmarks attached to entries.

Suggested columns:

- `entry_id TEXT PRIMARY KEY REFERENCES conversation_entry(id)`
- `label TEXT NOT NULL`
- `created_at_ms INTEGER`
- `metadata_json TEXT`

This can also be represented purely through `conversation_entry(kind = 'label')`, but a projection table keeps current labels cheap to query.
If we want strict append-only history semantics, the label entry remains canonical and this table is the latest-state projection.

### `conversation_fts`

SQLite FTS virtual table for lexical search.

Recommended first pass:

- FTS over `conversation_entry.search_text`
- include enough denormalized columns to rank/filter/group efficiently:
  - `entry_id`
  - `conversation_id`
  - `workspace_id`
  - `provider`
  - `kind`
  - `search_text`

Implementation options:

- contentless FTS table populated by importer/query layer
- external-content FTS table backed by `conversation_entry`

Recommended default:

- start with a contentless FTS table populated transactionally during import

Rationale:

- keeps FTS concerns explicit
- avoids surprises with trigger maintenance too early
- rebuild remains straightforward

### Tombstones / Missing Sources

Tombstone behavior should be driven primarily from `source_file.status`, plus `conversation.status` when needed.

Recommended default:

- keep entries and conversations in SQLite when sources disappear
- mark backing `source_file` rows as `missing` or `moved`
- exclude dead content from normal query results unless explicitly requested

## First Query Surface

The first schema should be sufficient to support these queries:

- list workspaces ordered by recent activity
- list conversations for a workspace
- search globally across conversations via entry-level FTS
- fetch a conversation transcript window by conversation ID
- fetch a transcript window centered around an entry ID
- show source/tombstone diagnostics for a conversation or workspace

## Search Model

### Indexed Search Unit

Search should operate on entry-level normalized content.

That gives us:

- better snippet targeting
- more precise ranking
- room to point results at meaningful locations inside a conversation

Entries of kind `thinking` should be preserved in storage but excluded from the default search index.

### Result Presentation

Even though indexing is entry-level, results should be grouped and presented at the conversation level by default.

Each result should carry:

- conversation identity
- workspace/project
- provider
- recency signals
- matched snippets
- optional stable entry identities for follow-up reads

### Ranking

Ranking should combine:

- text relevance
- recency

Neither should dominate completely.
The default behavior should reward recent relevant material without burying older high-value conversations.

### Noise Handling

The importer should preserve broadly normalized content.
Filtering should happen primarily at query time, not by aggressively discarding data at ingest time.

For transcript reads, the default rendering should be:

- user and assistant text inline
- tool calls inline
- tool results inline
- large tool outputs collapsed by default
- thinking blocks hidden or collapsed by default

## Read Model

Reads should come from SQLite, not from raw-file reparse on demand.

That means:

- search returns stable conversation identities
- it may also return stable entry identities
- `read` can fetch transcript windows or pages directly from normalized stored data

The default agent-facing read behavior should be windowed or paginated so large transcripts do not flood context.

## Why Not The Lighter-Weight Index

We explicitly considered a smaller derived index that would:

- store only metadata and searchable summaries
- locate raw transcript files
- hydrate transcript reads from raw files on demand

That remains a viable design in principle, but it loses force once we accept:

- a real sync mechanism is required anyway
- rebuild-on-schema-change is acceptable
- search and transcript reads are both central product features

At that point, a broad SQLite projection is simpler than maintaining:

- one thin custom index
- one separate raw-read path
- one custom mechanism for stitching them together at query time

## Non-Goals For This First Architecture

This architecture does not yet settle:

- semantic search implementation
- whether FTS alone is enough long-term
- whether background sync should eventually move into a separate service
- whether some importer logic should later be shared with or extracted from sibling tools

Those are important later decisions, but they are not blockers for the first SQLite-backed build.

## Identity Rules

The first implementation should use deterministic IDs derived from source provenance, never transcript content.

### Conversation Identity

- Prefer provider-native conversation/session IDs when they exist and are stable.
- Preserve the original provider ID separately so the browser can support copy-to-clipboard and provider-native resume actions.
- Derive the app-level `conversation.id` deterministically from `(provider, provider_conversation_id)` when a provider-native ID exists.
- If a provider-native ID does not exist, derive `conversation.id` from a provider-specific grouping key over source identity.
- Never derive conversation identity from transcript content.

### Entry Identity

- Prefer provider-native message/event IDs when they exist and are stable.
- Otherwise derive `conversation_entry.id` from deterministic source provenance:
  - `conversation_id`
  - `source_file_id`
  - source-local ordinal
- Source-local ordinal should mean the physical event order within the source transcript file.
- Never derive entry identity from entry content.

## Recommended First Build Slices

The architecture is now concrete enough to stop broad exploration and start landing thin vertical slices.

The next work should proceed in this order.

### Slice 1: SQLite Store And Schema Scaffolding

- add a dedicated storage module/crate boundary for SQLite access
- create the initial schema for:
  - `source_file`
  - `workspace`
  - `conversation`
  - `conversation_source_file`
  - `conversation_entry`
  - `entry_block`
  - `entry_label`
  - `conversation_fts`
- add a rebuild path so schema changes can wipe and recreate the derived store safely
- prove the schema opens cleanly and can be rebuilt repeatedly

Success criteria:

- the app can open an empty index
- the schema can be created and rebuilt idempotently
- storage-layer tests cover schema creation and reset behavior

### Slice 2: Importer Skeleton With Source Tracking

- add a provider-agnostic importer boundary
- persist `source_file` rows and sync bookkeeping first
- implement deterministic conversation and entry ID derivation rules
- import enough normalized data to populate:
  - workspaces
  - conversations
  - entry spine
  - minimal text/json blocks
- keep provider-specific provenance in metadata rather than inventing more core tables

Success criteria:

- Claude and Codex source files can be discovered and indexed into SQLite
- multi-file Codex conversations collapse into one logical conversation while preserving source provenance
- parse failures retain source-file context

### Slice 3: Query Layer And Basic Search

- add read/query APIs over SQLite for:
  - list workspaces
  - list conversations
  - fetch transcript windows by conversation ID
  - fetch transcript windows centered on an entry ID
  - global lexical search over indexed entries, grouped to conversations
- implement SQLite FTS population as part of import
- keep `thinking` out of default search results while preserving it in reads

Success criteria:

- CLI queries can serve the indexed data without touching raw transcript files
- search returns conversation-grouped results with stable conversation and entry IDs
- transcript reads come from stored normalized entries and blocks

### Slice 4: Startup Integration And Background Sync

- open SQLite on startup instead of loading transcripts directly into memory
- start from the last indexed state
- trigger incremental sync automatically in the background
- surface sync status in the TUI without blocking startup
- apply live updates as sync discovers changed transcripts

Success criteria:

- startup no longer reparses the full raw corpus before first render
- the app remains usable while sync catches up
- new transcript content appears without restart

### Slice 5: First Pi-Like History Browser Pass

- drive workspace and conversation navigation from SQLite-backed queries
- add the first tree/history browser over `conversation_entry`
- preserve heterogeneous entry kinds in transcript navigation
- support collapsible/non-default entry kinds such as `thinking`, large tool outputs, summaries, and compactions

Success criteria:

- transcript navigation is entry-aware rather than message-only
- the browser can render a first pass of Pi-like history navigation from indexed data
- current transcript-browser-specific customizations continue to work on top of the new store

## Implementation Notes

- Keep this execution order strict. Slice 3 depends on Slice 2 being real, and Slice 4 depends on Slice 3 providing usable reads.
- Do not pause to over-design durable migrations. Rebuild remains the escape hatch while the schema is still settling.
- Do not add a second search engine during these slices.
- Do not make transcript hydration fall back to raw-file parsing in the main read path once Slice 3 lands. The point of the store is to remove that coupling.

## Remaining Open Questions

Only a smaller set of design questions should remain open while implementation starts:

1. Which provider-native fields deserve first-class columns instead of JSON metadata after the first importer pass?
2. How much of `entry_block` should stay generic versus being split once real importer code exposes repeated access patterns?
3. What ranking formula over SQLite FTS and recency produces acceptable default search behavior?
4. Which transcript-browser-specific refinements should diverge first from Pi's history browser UX once the initial pass exists?
