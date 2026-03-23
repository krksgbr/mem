# Claude Run

**Date:** 2026-03-23
**Repo Name:** `claude-run`
**Category/Family:** Local Web UI / Session Manager Viewer (Read-only)

## Purpose
`claude-run` provides a beautiful, real-time web UI for browsing and monitoring Claude Code (Anthropic's official CLI tool) conversation histories. It acts as an external companion to the CLI, allowing developers to read their chat logs in a rich interface while continuing to interact with the agent in the terminal.

## Provider Coverage
Strictly **Claude Code** (Anthropic). It is deeply tied to the directory structure and specific JSONL output schema of Anthropic's CLI.

## Architecture Diagram

```text
                           +-------------------+
                           |   React Web UI    |
                           +--------+----------+
                                    |
                   (REST + Server-Sent Events - SSE)
                                    |
+-----------------------------------+-----------------------------------+
|                            Hono API Server                            |
|                                                                       |
|  +--------------------+   +-------------------+  +-----------------+  |
|  |  HTTP Endpoints    |   |  Chokidar Watcher |  | Memory Caches   |  |
|  |  (/api/sessions)   |   |  (fs.watch)       |  | (historyCache,  |  |
|  |  (/api/conv/:id)   +---+-------------------+  |  fileIndex)     |  |
|  +---------+----------+                          +---------+-------+  |
+------------|-----------------------------------------------|----------+
             | (Reads)                                       | (Watches)
+------------v-----------------------------------------------v----------+
|                        File System (~/.claude)                        |
|                                                                       |
|   history.jsonl                                                       |
|   projects/                                                           |
|     +- project-1/                                                     |
|     |  +- session_xyz.jsonl                                           |
|     +- project-2/                                                     |
|        +- session_abc.jsonl                                           |
+-----------------------------------------------------------------------+
```

## Ingestion, Storage, and Update Strategy

*   **Ingestion:** Reads directly from the `~/.claude/` directory structure. No external ingestion scripts are needed; it just parses the `history.jsonl` index and individual `projects/<project_name>/<session_id>.jsonl` files on demand.
*   **Storage:** The system is completely stateless. It does not employ a database like SQLite. Instead, it maintains a lightweight in-memory cache (`historyCache` and `fileIndex`) to map session IDs to their physical file paths and metadata.
*   **Update Mechanism:** Uses `chokidar` to actively watch the `~/.claude/history.jsonl` and `~/.claude/projects/` directories. File changes are debounced (20ms) and trigger callbacks that push updates over Server-Sent Events (SSE) to the React frontend.

## Transcript Structure Model

The model maps exactly to Claude Code's internal JSONL format. Messages are structured as a series of JSON objects appended to the log file.

```typescript
export interface ConversationMessage {
  type: "user" | "assistant" | "summary" | "file-history-snapshot";
  uuid?: string;
  parentUuid?: string;
  timestamp?: string;
  sessionId?: string;
  message?: {
    role: string;
    content: string | ContentBlock[];
    model?: string;
    usage?: TokenUsage;
  };
  summary?: string;
}

export interface ContentBlock {
  type: "text" | "thinking" | "tool_use" | "tool_result";
  text?: string;
  thinking?: string;
  id?: string;
  name?: string;
  input?: unknown;
  tool_use_id?: string;
  content?: string | ContentBlock[];
  is_error?: boolean;
}
```

## UX Shape

*   **Layout:** Standard side-by-side layout with a collapsible sidebar (session list) and main panel (conversation view).
*   **Real-time Streaming:** The UI updates dynamically via SSE as the CLI writes to the `.jsonl` files.
*   **Rich Tool Renderers:** The frontend has bespoke React components for specific tools utilized by the Claude Code agent (e.g., `BashRenderer`, `GrepRenderer`, `ReadRenderer`, `EditRenderer`).
*   **Read-Only Operations:** Users cannot send messages from the web UI. They can copy a "resume command" to jump back into the CLI session.
*   **Filtering:** Allows filtering sessions by project name or searching through prompts.

## Strengths

*   **Zero-Config/Zero-State:** Requires no migration, database setup, or syncing. Simply pointing it at `~/.claude` makes it instantly work.
*   **Smooth Live Streaming:** The combination of `chokidar` + SSE provides a fast, lightweight real-time viewing experience that complements the CLI workflow perfectly.
*   **Excellent Tool Presentation:** Abstracting CLI tool executions (`tool_use` / `tool_result`) into collapsible UI components vastly improves the readability of agent chains.

## Weaknesses

*   **Read-Only:** It fundamentally lacks write capabilities. It cannot be used as an alternate chat client.
*   **Scalability Limitations:** Relying on parsing flat JSONL files and in-memory indices means performance might degrade severely for power users with gigabytes of history, as it parses on demand via streams.
*   **Platform Lock-in:** It is completely hardcoded to Anthropic's proprietary CLI tool shapes and schema, meaning it cannot index other agents (like Codex) without significant refactoring.

## Key Code Snippets

**1. Live Streaming via SSE in Hono API:**
```typescript
app.get("/api/conversation/:id/stream", async (c) => {
  const sessionId = c.req.param("id");
  let offset = parseInt(c.req.query("offset") || "0", 10);

  return streamSSE(c, async (stream) => {
    const handleSessionChange = async (changedSessionId: string) => {
      if (changedSessionId !== sessionId) return;

      const { messages: newMessages, nextOffset: newOffset } =
        await getConversationStream(sessionId, offset);
      offset = newOffset;

      if (newMessages.length > 0) {
        await stream.writeSSE({
          event: "messages",
          data: JSON.stringify(newMessages),
        });
      }
    };
    onSessionChange(handleSessionChange);
    // ... setup heartbeat and stream logic
  });
});
```

**2. Incremental JSONL Stream Parser (`storage.ts`):**
```typescript
export async function getConversationStream(sessionId: string, fromOffset: number = 0): Promise<StreamResult> {
  const filePath = await findSessionFile(sessionId);
  // ...
  const fileHandle = await open(filePath, "r");
  const stream = fileHandle.createReadStream({ start: fromOffset, encoding: "utf-8" });
  const rl = createInterface({ input: stream, crlfDelay: Infinity });

  let bytesConsumed = 0;
  for await (const line of rl) {
    const lineBytes = Buffer.byteLength(line, "utf-8") + 1;
    if (line.trim()) {
      try {
        const msg: ConversationMessage = JSON.parse(line);
        if (msg.type === "user" || msg.type === "assistant") {
          messages.push(msg);
        }
        bytesConsumed += lineBytes;
      } catch { break; }
    } else {
      bytesConsumed += lineBytes;
    }
  }
  // ...
  return { messages, nextOffset };
}
```

**3. File System Watcher Callback (`watcher.ts`):**
```typescript
function emitChange(filePath: string): void {
  if (filePath.endsWith("history.jsonl")) {
    for (const callback of historyChangeListeners) {
      callback();
    }
  } else if (filePath.endsWith(".jsonl")) {
    const sessionId = basename(filePath, ".jsonl");
    for (const callback of sessionChangeListeners) {
      callback(sessionId, filePath);
    }
  }
}
```