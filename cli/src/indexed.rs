use crate::providers;
use crate::storage::Storage;
use anyhow::{Context, Result};
use rusqlite::{params, OptionalExtension, Transaction};
use serde::Serialize;
use serde_json::json;
use shared::{
    Conversation, ConversationLoadRef, ConversationSegment, Message, MessageKind, Participant,
    ProviderKind, Workspace,
};
use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Debug, PartialEq, Eq)]
struct SourceSnapshot {
    id: String,
    provider: ProviderKind,
    path: String,
    workspace_hint: Option<String>,
    file_size_bytes: u64,
    mtime_ms: i64,
}

#[derive(Clone, Debug)]
struct ImportedConversation {
    workspace: ImportedWorkspace,
    conversation: Conversation,
    sources: Vec<ImportedSourceLink>,
}

#[derive(Clone, Debug)]
struct ImportedWorkspace {
    id: String,
    display_name: String,
    canonical_path: Option<String>,
}

#[derive(Clone, Debug)]
struct ImportedSourceLink {
    source_file_id: String,
    role: String,
    ordinal: usize,
    metadata_json: String,
}

#[derive(Debug, Serialize)]
pub struct SearchResult {
    pub conversation_id: String,
    pub external_conversation_id: Option<String>,
    pub workspace: String,
    pub provider: String,
    pub title: String,
    pub snippet: String,
    pub matched_entry_id: String,
    pub updated_at_ms: i64,
}

#[derive(Debug, Serialize)]
pub struct ReadResult {
    pub conversation_id: String,
    pub external_conversation_id: Option<String>,
    pub title: String,
    pub provider: String,
    pub offset: usize,
    pub limit: usize,
    pub total_entries: usize,
    pub next_offset: Option<usize>,
    pub entries: Vec<ReadEntry>,
}

#[derive(Debug, Serialize)]
pub struct ReadEntry {
    pub entry_id: String,
    pub kind: String,
    pub participant: String,
    pub content: String,
    pub timestamp_ms: Option<i64>,
    pub depth: usize,
}

pub fn sync_now() -> Result<bool> {
    let mut storage = Storage::open_default()?;
    let sources = discover_sources()?;
    let changed = sync_storage(&mut storage, &sources)?;
    Ok(changed)
}

pub fn load_workspace_summaries() -> Result<Vec<Workspace>> {
    let storage = Storage::open_default()?;
    let conn = storage.raw_connection();

    let mut workspace_stmt = conn.prepare(
        r#"
        SELECT id, display_name, canonical_path, updated_at_ms
        FROM workspace
        WHERE status = 'active'
        ORDER BY updated_at_ms DESC, display_name ASC
        "#,
    )?;

    let workspace_rows = workspace_stmt.query_map([], |row| {
        Ok(Workspace {
            id: row.get(0)?,
            display_name: row.get(1)?,
            source_path: row.get(2)?,
            updated_at: row.get::<_, Option<i64>>(3)?.unwrap_or(0),
            conversations: Vec::new(),
        })
    })?;

    let mut workspaces: Vec<Workspace> = workspace_rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for workspace in &mut workspaces {
        workspace.conversations = load_conversation_summaries(conn, &workspace.id)?;
    }

    Ok(workspaces)
}

pub fn hydrate_conversation(conversation_id: &str) -> Result<Option<Conversation>> {
    let storage = Storage::open_default()?;
    load_full_conversation(storage.raw_connection(), conversation_id)
}

pub fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let sanitized = sanitize_fts_query(query);
    let storage = Storage::open_default()?;
    let conn = storage.raw_connection();

    let mut seen = BTreeSet::new();
    let mut results = Vec::new();

    if !sanitized.is_empty() {
        let mut stmt = conn.prepare(
            r#"
            SELECT
                c.id,
                c.provider_conversation_id,
                COALESCE(w.display_name, ''),
                c.provider,
                COALESCE(c.title, c.preview_text, c.id),
                snippet(conversation_fts, 2, '[', ']', ' … ', 18),
                conversation_fts.entry_id,
                COALESCE(c.updated_at_ms, 0)
            FROM conversation_fts
            JOIN conversation c ON c.id = conversation_fts.conversation_id
            LEFT JOIN workspace w ON w.id = c.workspace_id
            WHERE conversation_fts MATCH ?1 AND c.status = 'active'
            ORDER BY bm25(conversation_fts), c.updated_at_ms DESC
            LIMIT ?2
            "#,
        )?;

        let rows = stmt.query_map(params![sanitized, (limit.max(1) * 8) as i64], |row| {
            Ok(SearchResult {
                conversation_id: row.get(0)?,
                external_conversation_id: row.get(1)?,
                workspace: row.get(2)?,
                provider: row.get(3)?,
                title: row.get(4)?,
                snippet: row.get(5)?,
                matched_entry_id: row.get(6)?,
                updated_at_ms: row.get(7)?,
            })
        })?;

        for row in rows {
            let result = row?;
            if seen.insert(result.conversation_id.clone()) {
                results.push(result);
                if results.len() == limit.max(1) {
                    return Ok(results);
                }
            }
        }
    }

    let like_query = format!("%{query}%");
    let mut stmt = conn.prepare(
        r#"
        SELECT
            c.id,
            c.provider_conversation_id,
            COALESCE(w.display_name, ''),
            c.provider,
            COALESCE(c.title, c.preview_text, c.id),
            COALESCE(c.preview_text, c.title, c.id),
            '',
            COALESCE(c.updated_at_ms, 0)
        FROM conversation c
        LEFT JOIN workspace w ON w.id = c.workspace_id
        WHERE c.status = 'active'
          AND (COALESCE(c.title, '') LIKE ?1 OR COALESCE(c.preview_text, '') LIKE ?1)
        ORDER BY c.updated_at_ms DESC
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(params![like_query, (limit.max(1) * 4) as i64], |row| {
        Ok(SearchResult {
            conversation_id: row.get(0)?,
            external_conversation_id: row.get(1)?,
            workspace: row.get(2)?,
            provider: row.get(3)?,
            title: row.get(4)?,
            snippet: row.get(5)?,
            matched_entry_id: row.get(6)?,
            updated_at_ms: row.get(7)?,
        })
    })?;
    for row in rows {
        let result = row?;
        if seen.insert(result.conversation_id.clone()) {
            results.push(result);
            if results.len() == limit.max(1) {
                break;
            }
        }
    }

    Ok(results)
}

pub fn read(conversation_id: &str, offset: usize, limit: usize) -> Result<Option<ReadResult>> {
    let storage = Storage::open_default()?;
    let conn = storage.raw_connection();

    let Some(conversation) = load_full_conversation(conn, conversation_id)? else {
        return Ok(None);
    };

    let total_entries = conversation.messages.len();
    let start = offset.min(total_entries);
    let page_limit = limit.max(1);
    let end = (start + page_limit).min(total_entries);
    let entries = conversation.messages[start..end]
        .iter()
        .map(|entry| ReadEntry {
            entry_id: entry.id.clone().unwrap_or_default(),
            kind: message_kind_key(entry.kind).to_string(),
            participant: entry.participant.label(),
            content: entry.content.clone(),
            timestamp_ms: entry.timestamp,
            depth: entry.depth,
        })
        .collect();

    Ok(Some(ReadResult {
        conversation_id: conversation.id.clone(),
        external_conversation_id: conversation.external_id.clone(),
        title: conversation.display_title(),
        provider: provider_key(conversation.provider).to_string(),
        offset: start,
        limit: page_limit,
        total_entries,
        next_offset: (end < total_entries).then_some(end),
        entries,
    }))
}

pub fn spawn_background_sync() -> Receiver<Result<Option<Vec<Workspace>>>> {
    let (tx, rx) = mpsc::channel();

    thread::spawn(move || {
        let result = (|| -> Result<Option<Vec<Workspace>>> {
            let mut storage = Storage::open_default()?;
            let sources = discover_sources()?;
            let changed = sync_storage(&mut storage, &sources)?;
            if !changed {
                return Ok(None);
            }

            let workspaces = load_workspace_summaries()?;
            Ok(Some(workspaces))
        })();

        let _ = tx.send(result);
    });

    rx
}

fn sync_storage(storage: &mut Storage, sources: &[SourceSnapshot]) -> Result<bool> {
    let existing = load_existing_sources(storage.raw_connection())?;
    if !sources_changed(&existing, sources) {
        return Ok(false);
    }

    import_all(storage.raw_connection_mut(), sources)?;
    Ok(true)
}

fn import_all(conn: &mut rusqlite::Connection, sources: &[SourceSnapshot]) -> Result<()> {
    let mut imported = Vec::new();
    imported.extend(import_claude_workspaces()?);
    imported.extend(import_codex_workspaces()?);

    let now_ms = now_ms();
    let tx = conn
        .transaction()
        .context("failed to start import transaction")?;

    clear_index_content(&tx)?;
    insert_sources(&tx, sources, now_ms)?;
    insert_workspaces(&tx, &imported, now_ms)?;
    insert_conversations(&tx, &imported)?;
    insert_entries(&tx, &imported)?;
    tx.commit().context("failed to commit import transaction")?;
    Ok(())
}

fn clear_index_content(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO conversation_fts(conversation_fts) VALUES ('delete-all')",
        [],
    )
    .context("failed to clear contentless FTS index")?;

    for statement in [
        "DELETE FROM entry_label",
        "DELETE FROM entry_block",
        "DELETE FROM conversation_entry",
        "DELETE FROM conversation_source_file",
        "DELETE FROM conversation",
        "DELETE FROM workspace",
        "DELETE FROM source_file",
    ] {
        tx.execute(statement, []).with_context(|| {
            format!("failed to clear indexed content with statement: {statement}")
        })?;
    }

    Ok(())
}

fn discover_sources() -> Result<Vec<SourceSnapshot>> {
    let mut sources = Vec::new();
    let home_dir = std::env::var("HOME").context("HOME is not set")?;

    let claude_base = PathBuf::from(&home_dir).join(".claude/projects");
    if claude_base.exists() {
        collect_jsonl_files(&claude_base, ProviderKind::ClaudeCode, &mut sources)?;
    }

    let codex_base = PathBuf::from(&home_dir).join(".codex/sessions");
    if codex_base.exists() {
        collect_jsonl_files(&codex_base, ProviderKind::Codex, &mut sources)?;
    }

    sources.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(sources)
}

fn collect_jsonl_files(
    root: &Path,
    provider: ProviderKind,
    out: &mut Vec<SourceSnapshot>,
) -> Result<()> {
    for entry in fs::read_dir(root)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_jsonl_files(&path, provider, out)?;
            continue;
        }

        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let metadata = fs::metadata(&path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?;
        let mtime = metadata
            .modified()
            .ok()
            .and_then(system_time_to_ms)
            .unwrap_or(0);
        let canonical = path.to_string_lossy().to_string();

        out.push(SourceSnapshot {
            id: source_file_id(provider, &canonical),
            provider,
            path: canonical,
            workspace_hint: None,
            file_size_bytes: metadata.len(),
            mtime_ms: mtime,
        });
    }

    Ok(())
}

fn load_existing_sources(
    conn: &rusqlite::Connection,
) -> Result<BTreeMap<String, (String, i64, u64, String)>> {
    let mut stmt = conn.prepare(
        "SELECT path, provider, mtime_ms, file_size_bytes, status FROM source_file ORDER BY path",
    )?;
    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<i64>>(2)?.unwrap_or(0),
            row.get::<_, Option<u64>>(3)?.unwrap_or(0),
            row.get::<_, String>(4)?,
        ))
    })?;

    let mut by_path = BTreeMap::new();
    for row in rows {
        let (path, provider, mtime_ms, size, status) = row?;
        by_path.insert(path, (provider, mtime_ms, size, status));
    }
    Ok(by_path)
}

fn sources_changed(
    existing: &BTreeMap<String, (String, i64, u64, String)>,
    current: &[SourceSnapshot],
) -> bool {
    if existing.len() != current.len() {
        return true;
    }

    for source in current {
        let Some((provider, mtime_ms, size, status)) = existing.get(&source.path) else {
            return true;
        };
        if *provider != provider_key(source.provider)
            || *mtime_ms != source.mtime_ms
            || *size != source.file_size_bytes
            || status != "active"
        {
            return true;
        }
    }

    false
}

fn import_claude_workspaces() -> Result<Vec<ImportedConversation>> {
    let workspaces = providers::claude::load_workspaces_full()?;
    Ok(flatten_workspaces(workspaces))
}

fn import_codex_workspaces() -> Result<Vec<ImportedConversation>> {
    let workspaces = providers::codex::load_workspaces_full()?;
    Ok(flatten_workspaces(workspaces))
}

fn flatten_workspaces(workspaces: Vec<Workspace>) -> Vec<ImportedConversation> {
    let mut imported = Vec::new();

    for workspace in workspaces {
        let workspace_id = normalized_workspace_id(&workspace);
        let workspace_row = ImportedWorkspace {
            id: workspace_id,
            display_name: workspace.display_name.clone(),
            canonical_path: workspace.source_path.clone(),
        };

        for mut conversation in workspace.conversations {
            normalize_conversation_identity(&workspace_row.id, &mut conversation);
            let sources = conversation_sources(&conversation);
            imported.push(ImportedConversation {
                workspace: workspace_row.clone(),
                conversation,
                sources,
            });
        }
    }

    imported
}

fn normalize_conversation_identity(workspace_id: &str, conversation: &mut Conversation) {
    let provider_native_id = conversation
        .external_id
        .clone()
        .unwrap_or_else(|| conversation.id.clone());
    conversation.id = format!(
        "{}:{}:{}",
        provider_key(conversation.provider),
        workspace_id,
        provider_native_id
    );
}

fn normalized_workspace_id(workspace: &Workspace) -> String {
    if let Some(source_path) = workspace.source_path.as_ref() {
        return source_path.clone();
    }

    normalize_workspace_display_name(&workspace.display_name)
        .unwrap_or_else(|| workspace.display_name.clone())
}

fn normalize_workspace_display_name(display_name: &str) -> Option<String> {
    if let Some(rest) = display_name.strip_prefix("~/") {
        let home_dir = env::var("HOME").ok()?;
        return Some(format!("{home_dir}/{rest}"));
    }

    if display_name.starts_with('/') {
        return Some(display_name.to_string());
    }

    None
}

fn conversation_sources(conversation: &Conversation) -> Vec<ImportedSourceLink> {
    match conversation.load_ref.as_ref() {
        Some(ConversationLoadRef::ClaudeFile { path }) => vec![ImportedSourceLink {
            source_file_id: source_file_id(ProviderKind::ClaudeCode, path),
            role: "primary".into(),
            ordinal: 0,
            metadata_json: "{}".into(),
        }],
        Some(ConversationLoadRef::CodexFiles { paths }) => paths
            .iter()
            .enumerate()
            .map(|(idx, path)| {
                let metadata_json = conversation
                    .segments
                    .get(idx)
                    .map(|segment| {
                        json!({
                            "segment_id": segment.id,
                            "segment_label": segment.label,
                            "created_at_ms": segment.created_at,
                            "updated_at_ms": segment.updated_at,
                            "message_start_idx": segment.message_start_idx,
                            "message_count": segment.message_count,
                        })
                        .to_string()
                    })
                    .unwrap_or_else(|| "{}".into());

                ImportedSourceLink {
                    source_file_id: source_file_id(ProviderKind::Codex, path),
                    role: if idx == 0 {
                        "primary".into()
                    } else {
                        "segment".into()
                    },
                    ordinal: idx,
                    metadata_json,
                }
            })
            .collect(),
        Some(ConversationLoadRef::Indexed { .. }) | None => Vec::new(),
    }
}

fn insert_sources(tx: &Transaction<'_>, sources: &[SourceSnapshot], now_ms: i64) -> Result<()> {
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO source_file (
            id, provider, path, status, workspace_hint, file_size_bytes, mtime_ms,
            last_indexed_at_ms, last_seen_at_ms, provider_metadata_json
        ) VALUES (?1, ?2, ?3, 'active', ?4, ?5, ?6, ?7, ?7, ?8)
        "#,
    )?;

    for source in sources {
        stmt.execute(params![
            source.id,
            provider_key(source.provider),
            source.path,
            source.workspace_hint,
            source.file_size_bytes as i64,
            source.mtime_ms,
            now_ms,
            "{}",
        ])?;
    }

    Ok(())
}

fn insert_workspaces(
    tx: &Transaction<'_>,
    imported: &[ImportedConversation],
    now_ms: i64,
) -> Result<()> {
    let mut seen = BTreeSet::new();
    let mut stmt = tx.prepare(
        r#"
        INSERT INTO workspace (
            id, canonical_path, display_name, provider_scope, status, created_at_ms, updated_at_ms, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, 'active', ?5, ?6, ?7)
        "#,
    )?;

    for row in imported {
        if !seen.insert(row.workspace.id.clone()) {
            continue;
        }

        stmt.execute(params![
            row.workspace.id,
            row.workspace.canonical_path,
            row.workspace.display_name,
            Option::<String>::None,
            now_ms,
            row.conversation.updated_at,
            "{}",
        ])?;
    }

    Ok(())
}

fn insert_conversations(tx: &Transaction<'_>, imported: &[ImportedConversation]) -> Result<()> {
    let mut conv_stmt = tx.prepare(
        r#"
        INSERT INTO conversation (
            id, workspace_id, provider, provider_conversation_id, title, preview_text, status,
            created_at_ms, updated_at_ms, last_source_event_at_ms, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'active', ?7, ?8, ?9, ?10)
        "#,
    )?;
    let mut source_stmt = tx.prepare(
        r#"
        INSERT INTO conversation_source_file (
            conversation_id, source_file_id, role, ordinal, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5)
        "#,
    )?;

    for row in imported {
        let provider_conversation_id = row
            .conversation
            .external_id
            .as_ref()
            .cloned()
            .unwrap_or_else(|| row.conversation.id.clone());
        conv_stmt.execute(params![
            row.conversation.id,
            row.workspace.id,
            provider_key(row.conversation.provider),
            provider_conversation_id,
            row.conversation.title,
            row.conversation.preview,
            row.conversation.created_at,
            row.conversation.updated_at,
            row.conversation.updated_at,
            "{}",
        ])?;

        for source in &row.sources {
            source_stmt.execute(params![
                row.conversation.id,
                source.source_file_id,
                source.role,
                source.ordinal as i64,
                source.metadata_json,
            ])?;
        }
    }

    Ok(())
}

fn insert_entries(tx: &Transaction<'_>, imported: &[ImportedConversation]) -> Result<()> {
    let mut entry_stmt = tx.prepare(
        r#"
        INSERT INTO conversation_entry (
            id, conversation_id, kind, parent_entry_id, associated_entry_id, source_file_id,
            provider_entry_id, ordinal, timestamp_ms, is_searchable, search_text, summary_text, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
        "#,
    )?;
    let mut block_stmt = tx.prepare(
        r#"
        INSERT INTO entry_block (
            id, entry_id, ordinal, kind, text_value, json_value, mime_type, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
    )?;
    let mut fts_stmt = tx.prepare(
        "INSERT INTO conversation_fts (entry_id, conversation_id, search_text) VALUES (?1, ?2, ?3)",
    )?;

    for row in imported {
        for (ordinal, message) in row.conversation.messages.iter().enumerate() {
            let entry_id = message
                .id
                .clone()
                .unwrap_or_else(|| format!("{}:{ordinal}", row.conversation.id));
            let source_file_id = row
                .sources
                .first()
                .map(|source| source.source_file_id.clone());
            let search_text = searchable_text(message);
            let participant_label = message.participant.label();

            entry_stmt.execute(params![
                entry_id,
                row.conversation.id,
                message_kind_key(message.kind),
                message.parent_id,
                message.associated_id,
                source_file_id,
                message.id,
                ordinal as i64,
                message.timestamp,
                if message.kind.is_searchable_by_default() {
                    1
                } else {
                    0
                },
                search_text,
                participant_label,
                json!({
                    "participant": participant_label,
                    "depth": message.depth,
                })
                .to_string(),
            ])?;

            block_stmt.execute(params![
                format!("{entry_id}:block:0"),
                entry_id,
                0_i64,
                "text",
                message.content,
                Option::<String>::None,
                Option::<String>::None,
                "{}",
            ])?;

            if let Some(search_text) = search_text {
                fts_stmt.execute(params![entry_id, row.conversation.id, search_text])?;
            }
        }
    }

    Ok(())
}

fn load_conversation_summaries(
    conn: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, provider_conversation_id, provider, title, preview_text, created_at_ms, updated_at_ms
        FROM conversation
        WHERE workspace_id = ?1 AND status = 'active'
        ORDER BY updated_at_ms DESC, id ASC
        "#,
    )?;
    let rows = stmt.query_map([workspace_id], |row| {
        Ok(Conversation {
            id: row.get(0)?,
            external_id: row.get(1)?,
            title: row.get(3)?,
            preview: row.get(4)?,
            provider: parse_provider_key(&row.get::<_, String>(2)?),
            created_at: row.get::<_, Option<i64>>(5)?.unwrap_or(0),
            updated_at: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            segments: Vec::new(),
            messages: Vec::new(),
            is_hydrated: false,
            load_ref: Some(ConversationLoadRef::Indexed {
                conversation_id: row.get(0)?,
            }),
        })
    })?;

    let mut conversations = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for conversation in &mut conversations {
        conversation.segments = load_segments(conn, &conversation.id)?;
    }
    Ok(conversations)
}

fn load_segments(
    conn: &rusqlite::Connection,
    conversation_id: &str,
) -> Result<Vec<ConversationSegment>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT metadata_json
        FROM conversation_source_file
        WHERE conversation_id = ?1
        ORDER BY ordinal ASC
        "#,
    )?;

    let rows = stmt.query_map([conversation_id], |row| row.get::<_, Option<String>>(0))?;
    let mut segments = Vec::new();
    for row in rows {
        let Some(metadata_json) = row? else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&metadata_json) else {
            continue;
        };
        let Some(segment_id) = value.get("segment_id").and_then(|v| v.as_str()) else {
            continue;
        };
        let Some(label) = value.get("segment_label").and_then(|v| v.as_str()) else {
            continue;
        };
        segments.push(ConversationSegment {
            id: segment_id.to_string(),
            label: label.to_string(),
            created_at: value
                .get("created_at_ms")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            updated_at: value
                .get("updated_at_ms")
                .and_then(|v| v.as_i64())
                .unwrap_or(0),
            message_start_idx: value
                .get("message_start_idx")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
            message_count: value
                .get("message_count")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize,
        });
    }
    Ok(segments)
}

fn load_full_conversation(
    conn: &rusqlite::Connection,
    conversation_id: &str,
) -> Result<Option<Conversation>> {
    let conversation = conn
        .query_row(
            r#"
            SELECT id, workspace_id, provider_conversation_id, provider, title, preview_text,
                   created_at_ms, updated_at_ms
            FROM conversation
            WHERE id = ?1 AND status = 'active'
            "#,
            [conversation_id],
            |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    external_id: row.get(2)?,
                    title: row.get(4)?,
                    preview: row.get(5)?,
                    provider: parse_provider_key(&row.get::<_, String>(3)?),
                    created_at: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
                    updated_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    segments: Vec::new(),
                    messages: Vec::new(),
                    is_hydrated: true,
                    load_ref: Some(ConversationLoadRef::Indexed {
                        conversation_id: row.get(0)?,
                    }),
                })
            },
        )
        .optional()?;

    let Some(mut conversation) = conversation else {
        return Ok(None);
    };

    conversation.segments = load_segments(conn, &conversation.id)?;
    conversation.messages = load_messages(conn, &conversation.id)?;
    Ok(Some(conversation))
}

fn load_messages(conn: &rusqlite::Connection, conversation_id: &str) -> Result<Vec<Message>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, kind, provider_entry_id, parent_entry_id, associated_entry_id, timestamp_ms, summary_text,
               metadata_json,
               COALESCE((SELECT text_value FROM entry_block WHERE entry_id = conversation_entry.id ORDER BY ordinal ASC LIMIT 1), search_text, '')
        FROM conversation_entry
        WHERE conversation_id = ?1
        ORDER BY ordinal ASC
        "#,
    )?;

    let rows = stmt.query_map([conversation_id], |row| {
        let summary_text: Option<String> = row.get(6)?;
        let metadata_json: Option<String> = row.get(7)?;
        let participant = metadata_json
            .as_deref()
            .and_then(participant_from_metadata)
            .unwrap_or(Participant::System);
        let depth = metadata_json
            .as_deref()
            .and_then(depth_from_metadata)
            .unwrap_or(0);
        let content: String = row.get(8)?;
        let content = if content.is_empty() {
            summary_text.clone().unwrap_or_default()
        } else {
            content
        };

        Ok(Message {
            id: row.get::<_, Option<String>>(0)?,
            kind: parse_message_kind(&row.get::<_, String>(1)?),
            participant,
            content,
            timestamp: row.get(5)?,
            parent_id: row.get(3)?,
            associated_id: row.get(4)?,
            depth,
        })
    })?;

    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn participant_from_metadata(metadata_json: &str) -> Option<Participant> {
    let value = serde_json::from_str::<serde_json::Value>(metadata_json).ok()?;
    match value.get("participant").and_then(|v| v.as_str())? {
        "You" => Some(Participant::User),
        "Claude Code" => Some(Participant::Assistant {
            provider: ProviderKind::ClaudeCode,
        }),
        "Codex" => Some(Participant::Assistant {
            provider: ProviderKind::Codex,
        }),
        "System" => Some(Participant::System),
        other => Some(Participant::Unknown {
            raw_role: other.to_string(),
        }),
    }
}

fn depth_from_metadata(metadata_json: &str) -> Option<usize> {
    let value = serde_json::from_str::<serde_json::Value>(metadata_json).ok()?;
    value.get("depth")?.as_u64().map(|value| value as usize)
}

fn searchable_text(message: &Message) -> Option<String> {
    if !message.kind.is_searchable_by_default() {
        return None;
    }

    let trimmed = message.content.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn sanitize_fts_query(query: &str) -> String {
    query
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
        .map(|token| format!("\"{token}\""))
        .collect::<Vec<_>>()
        .join(" ")
}

fn source_file_id(provider: ProviderKind, path: &str) -> String {
    format!("{}:{path}", provider_key(provider))
}

fn provider_key(provider: ProviderKind) -> &'static str {
    match provider {
        ProviderKind::ClaudeCode => "claude_code",
        ProviderKind::Codex => "codex",
    }
}

fn parse_provider_key(value: &str) -> ProviderKind {
    match value {
        "codex" => ProviderKind::Codex,
        _ => ProviderKind::ClaudeCode,
    }
}

fn message_kind_key(kind: MessageKind) -> &'static str {
    match kind {
        MessageKind::UserMessage => "user_message",
        MessageKind::AssistantMessage => "assistant_message",
        MessageKind::ToolCall => "tool_call",
        MessageKind::ToolResult => "tool_result",
        MessageKind::Thinking => "thinking",
        MessageKind::Summary => "summary",
        MessageKind::Compaction => "compaction",
        MessageKind::Label => "label",
        MessageKind::MetadataChange => "metadata_change",
    }
}

fn parse_message_kind(value: &str) -> MessageKind {
    match value {
        "user_message" => MessageKind::UserMessage,
        "assistant_message" => MessageKind::AssistantMessage,
        "tool_call" => MessageKind::ToolCall,
        "tool_result" => MessageKind::ToolResult,
        "thinking" => MessageKind::Thinking,
        "summary" => MessageKind::Summary,
        "compaction" => MessageKind::Compaction,
        "label" => MessageKind::Label,
        _ => MessageKind::MetadataChange,
    }
}

fn system_time_to_ms(time: SystemTime) -> Option<i64> {
    time.duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
}

fn now_ms() -> i64 {
    system_time_to_ms(SystemTime::now()).unwrap_or(0)
}
