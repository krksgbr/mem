# Product Design Direction: Agent Memory Manager & Interconnect

## Vision
The `transcript-browser` is evolving beyond a passive log viewer into an **active memory interconnect and management plane** for local AI agents (Claude Code, Codex, etc.). It serves two distinct but overlapping constituencies: **Humans** (needing to manage, prune, and organize disk-based histories) and **Agents** (needing to discover, search, and ingest past context across different providers and projects).

## Core Triggers & Use Cases

### 1. Garbage Collection (GC) & Noise Reduction
* **The Problem:** I often need to re-login with claude code and it generates empty conversations containing only the login flow. there are some other instances where useless conversations are presisted and i want to be able to quickly find and garbage collect these so they don't clutter up my conversation history.
* **The Workflow:** A highly scannable TUI list view that surfaces metadata (turn count, duration, summary). The user can quickly identify low-value sessions and permanently delete them from disk with rapid keyboard shortcuts (e.g., `Ctrl+D`).

### 2. Project Migration & History Management
* **The Problem:** Somewhat related to the garbage collection idea that I forgot: moving/renaming projects. In claude code, transcripts are tightly coupled to the project's path, so if I move a project to a new disk location, I basically lose access to its history. not sure how it works in other agents, but i'd want to use this tool for these sorts of organization/cleanup tasks.
* **The Workflow:** A management interface that detects orphaned histories, lists known projects, and provides tools to re-map or rename transcript storage directories to match new project locations.

### 3. The Agent "Address Book" (Cross-Context Sharing)
* **The Problem:** When working with an agent on something, like designing a specification or trying to solve an implementation problem, i often find myself wanting to reference conversations across projects and agents. say i'm talking to codex and a topic comes up which i know i discussed with claude code 2 weeks ago and I'd want to tell codex: "this is something I discussed with claude code a couple weeks back, i think it was in project <project-name>, can you search the transcript and see if you find anything relevant to our current discussion?"
* **The Workflow:** The tool acts as a searchable directory. A user can tell an active agent: *"oh i've talked about this at length with claude code yesterday. can you find the convo and bounce your ideas off?"* The active agent uses the tool (via CLI or MCP) to search across all normalized local histories, read the relevant transcript, and integrate those insights into the current task.

### 4. Cross-Agent Conversations
* **The Problem:** I'd like this tool to be useful for cross-agent conversations. sometimes I want different agents to talk to each other, to share information. or two instances of the same agent, talking to each other across projects, or intra-project. like let's say I discuss topic A with claude code in a project, and have a parallel conversation on topic B, inside the same project. at some point I realize the topics are related and I want them to have a structured discussion, sharing insights only they have due to their particular contexts.
* **The Workflow:** The tool provides the "address" of sessions so agents can find each other and read/search each other's transcripts to facilitate these structured discussions. While live pipes/connections are a future direction, the focus now is on shared historical memory.

---

## The "Two Products" Paradigm

While unified under one tool, the feature set effectively splits into two distinct operational modes:

### Mode A: The Workspace Manager (Human UX)
* **Target Audience:** The Developer.
* **Interface:** Terminal UI (TUI).
* **Focus:** File-system operations, maintenance, and organization.
* **Key Features:** 
  * Bulk deletion / fast GC.
  * Project directory re-linking.
  * Visual scanning of recent cross-agent activity.

### Mode B: The Memory Interconnect (Agent UX)
* **Target Audience:** The Agents (via LLM tool use).
* **Interface:** Headless CLI / JSON output / MCP Server.
* **Focus:** Search, retrieval, and schema normalization.
* **Key Features:**
  * Provider-agnostic querying (searching across Claude, Codex, etc., seamlessly).
  * Lossless normalization (exposing thoughts, tool inputs, and tool outputs in a flat, digestible format so the querying agent understands *how* a problem was solved, not just the final text).
  * **Out of Scope (For Now):** Facilitating live pipes/connections between actively running agents. The focus remains on reading historical/static transcripts.

---

## Architectural Implications

Moving forward, `transcript-browser` will be built as a **unified, pure Rust tool**. It will handle both the Management (Mode A) and the Search (Mode B) workflows within a single, high-performance binary.

While heavy-duty indexers like `memex` focus strictly on read-only search and embeddings, `transcript-browser` differentiates itself by integrating deep file-system management capabilities directly alongside search. By leveraging Rust's performance and ecosystem (e.g., SQLite for fast local indexing, `ratatui` for the TUI), it will provide a fast, dependency-free experience that fully satisfies both Human GC/Migration and Agent Memory Retrieval use cases without needing to run separate indexing services.
