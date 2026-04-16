use crate::providers;
use crate::storage::Storage;
use anyhow::{Context, Result};
use regex::Regex;
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
use std::path::{Component, Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::LazyLock;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

const TITLE_WEIGHT: f64 = 12.0;
const PREVIEW_WEIGHT: f64 = 1.0;
const OPENING_PROMPT_WEIGHT: f64 = 10.0;
const EARLY_CONTEXT_WEIGHT: f64 = 4.0;
const ARTIFACT_WEIGHT: f64 = 4.0;
const BODY_WEIGHT: f64 = 1.0;
const NL_TITLE_WEIGHT: f64 = 3.0;
const NL_PREVIEW_WEIGHT: f64 = 1.0;
const NL_OPENING_PROMPT_WEIGHT: f64 = 12.0;
const NL_EARLY_CONTEXT_WEIGHT: f64 = 6.0;
const NL_ARTIFACT_WEIGHT: f64 = 0.0;
const NL_BODY_WEIGHT: f64 = 0.5;
const QUERY_STOPWORDS: &[&str] = &[
    "a",
    "an",
    "the",
    "ago",
    "around",
    "about",
    "couple",
    "day",
    "days",
    "discuss",
    "discussed",
    "find",
    "for",
    "in",
    "into",
    "of",
    "on",
    "project",
    "talked",
    "that",
    "this",
    "we",
    "where",
];

static BACKTICK_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`\n]+)`").expect("valid backtick regex"));
static ABSOLUTE_PATH_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<path>(?:~|/)[A-Za-z0-9._/\-]+)").expect("valid path regex"));
static FLAG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<flag>--[A-Za-z0-9][A-Za-z0-9._\-]*)").expect("valid flag regex")
});
static SLASH_COMMAND_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<cmd>/[A-Za-z0-9][A-Za-z0-9._\-]*)").expect("valid slash command regex")
});
static DOTTED_IDENTIFIER_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<ident>[A-Za-z_][A-Za-z0-9_]*(?:\.[A-Za-z0-9_]+){1,})")
        .expect("valid dotted identifier regex")
});
static WORD_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?P<token>[A-Za-z][A-Za-z0-9]+)").expect("valid word regex"));
static CODELIKE_TOKEN_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?P<token>[A-Za-z0-9]+(?:[_\-][A-Za-z0-9]+)+)")
        .expect("valid codelike token regex")
});

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
    pub is_branch: bool,
    pub branch_parent_conversation_id: Option<String>,
    pub branch_parent_title: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct WorkspaceListResult {
    pub workspace_id: String,
    pub display_name: String,
    pub canonical_path: Option<String>,
    pub updated_at_ms: i64,
    pub conversation_count: usize,
    pub claude_code_conversation_count: usize,
    pub codex_conversation_count: usize,
}

#[derive(Debug, Serialize)]
pub struct RecentConversationResult {
    pub conversation_id: String,
    pub external_conversation_id: Option<String>,
    pub workspace: String,
    pub workspace_id: String,
    pub provider: String,
    pub title: String,
    pub snippet: String,
    pub updated_at_ms: i64,
    pub is_branch: bool,
    pub branch_parent_conversation_id: Option<String>,
    pub branch_parent_title: Option<String>,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchContext {
    exact_tokens: Vec<String>,
    nl_tokens: Vec<String>,
    artifact_terms: Vec<String>,
    corrected_artifact_terms: Option<Vec<String>>,
    workspace_id_hint: Option<String>,
    workspace_id_filter: Option<String>,
    recent_cutoff_ms: Option<i64>,
    prefers_exact_surface: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct WorkspaceSearchHint {
    workspace_id: String,
    matches: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchSurface {
    Exact,
    NaturalLanguage,
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

pub fn list_workspaces(provider: Option<ProviderKind>) -> Result<Vec<WorkspaceListResult>> {
    let workspaces = load_workspace_summaries()?;
    Ok(workspaces
        .into_iter()
        .filter_map(|workspace| {
            let conversation_count = workspace
                .conversations
                .iter()
                .filter(|conversation| provider.is_none_or(|p| conversation.provider == p))
                .count();
            let claude_code_conversation_count = workspace
                .conversations
                .iter()
                .filter(|conversation| conversation.provider == ProviderKind::ClaudeCode)
                .count();
            let codex_conversation_count = workspace
                .conversations
                .iter()
                .filter(|conversation| conversation.provider == ProviderKind::Codex)
                .count();

            (provider.is_none() || conversation_count > 0).then_some(WorkspaceListResult {
                workspace_id: workspace.id,
                display_name: workspace.display_name,
                canonical_path: workspace.source_path,
                updated_at_ms: workspace.updated_at,
                conversation_count,
                claude_code_conversation_count,
                codex_conversation_count,
            })
        })
        .collect())
}

pub fn latest_conversations(
    provider: Option<ProviderKind>,
    workspace_selector: Option<&str>,
    limit: usize,
) -> Result<Vec<RecentConversationResult>> {
    let workspaces = load_workspace_summaries()?;
    let workspace_selector = workspace_selector.map(|value| value.trim());

    let mut results = workspaces
        .into_iter()
        .filter(|workspace| {
            workspace_selector.is_none_or(|selector| {
                selector == workspace.id
                    || selector == workspace.display_name
                    || workspace.source_path.as_deref() == Some(selector)
            })
        })
        .flat_map(|workspace| {
            let workspace_id = workspace.id.clone();
            let workspace_name = workspace.display_name.clone();
            workspace
                .conversations
                .into_iter()
                .filter(move |conversation| provider.is_none_or(|p| conversation.provider == p))
                .map(move |conversation| RecentConversationResult {
                    conversation_id: conversation.id.clone(),
                    external_conversation_id: conversation.external_id.clone(),
                    workspace: workspace_name.clone(),
                    workspace_id: workspace_id.clone(),
                    provider: provider_key(conversation.provider).to_string(),
                    title: conversation.display_title(),
                    snippet: conversation
                        .latest_activity_line()
                        .map(str::to_string)
                        .unwrap_or_else(|| conversation.display_title()),
                    updated_at_ms: conversation.updated_at,
                    is_branch: conversation.branch_parent_id.is_some(),
                    branch_parent_conversation_id: conversation.branch_parent_id.clone(),
                    branch_parent_title: None,
                })
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| {
        right
            .updated_at_ms
            .cmp(&left.updated_at_ms)
            .then_with(|| left.title.cmp(&right.title))
    });
    results.truncate(limit.max(1));
    Ok(results)
}

pub fn search(query: &str, limit: usize) -> Result<Vec<SearchResult>> {
    let storage = Storage::open_default()?;
    let conn = storage.raw_connection();
    let search_context = build_search_context(conn, query)?;

    let mut seen = BTreeSet::new();
    let mut results = Vec::new();
    let surface_order = if search_context.prefers_exact_surface {
        [SearchSurface::Exact, SearchSurface::NaturalLanguage]
    } else {
        [SearchSurface::NaturalLanguage, SearchSurface::Exact]
    };
    for surface in surface_order {
        let tier_queries = match surface {
            SearchSurface::Exact if !search_context.exact_tokens.is_empty() => {
                build_exact_tiers(
                    &search_context.exact_tokens,
                    &search_context.artifact_terms,
                    search_context.corrected_artifact_terms.as_deref(),
                )
            }
            SearchSurface::NaturalLanguage if !search_context.nl_tokens.is_empty() => {
                build_nl_fts_search_tiers(&search_context.nl_tokens)
            }
            _ => Vec::new(),
        };
        for tier_query in tier_queries {
            let tier_results = execute_fts_search_tier(
                conn,
                &tier_query,
                &search_context,
                surface,
                limit.max(1) * 8,
            )?;
            for result in tier_results {
                if seen.insert(result.conversation_id.clone()) {
                    results.push(result);
                    if results.len() == limit.max(1) {
                        return Ok(results);
                    }
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
            c.preview_text,
            COALESCE(c.preview_text, c.title, c.id),
            '',
            COALESCE(c.updated_at_ms, 0),
            c.parent_conversation_id,
            parent.title
        FROM conversation c
        LEFT JOIN workspace w ON w.id = c.workspace_id
        LEFT JOIN conversation parent ON parent.id = c.parent_conversation_id
        WHERE c.status = 'active'
          AND (?3 IS NULL OR c.workspace_id = ?3)
          AND (COALESCE(c.title, '') LIKE ?1 OR COALESCE(c.preview_text, '') LIKE ?1)
        ORDER BY c.updated_at_ms DESC
        LIMIT ?2
        "#,
    )?;
    let rows = stmt.query_map(
        params![
            like_query,
            (limit.max(1) * 4) as i64,
            search_context.workspace_id_filter
        ],
        |row| {
            let title: String = row.get(4)?;
            let preview_text: Option<String> = row.get(5)?;
            let matched_snippet: String = row.get(6)?;
            Ok(SearchResult {
                conversation_id: row.get(0)?,
                external_conversation_id: row.get(1)?,
                workspace: row.get(2)?,
                provider: row.get(3)?,
                title: title.clone(),
                snippet: choose_search_snippet(&title, preview_text.as_deref(), &matched_snippet),
                matched_entry_id: row.get(7)?,
                updated_at_ms: row.get(8)?,
                is_branch: row.get::<_, Option<String>>(9)?.is_some(),
                branch_parent_conversation_id: row.get(9)?,
                branch_parent_title: row.get(10)?,
            })
        },
    )?;
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

    let Some(conversation_id) = resolve_conversation_selector(conn, conversation_id)? else {
        return Ok(None);
    };

    let Some(conversation) = load_full_conversation(conn, &conversation_id)? else {
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

fn resolve_conversation_selector(
    conn: &rusqlite::Connection,
    selector: &str,
) -> Result<Option<String>> {
    let exact_id = conn
        .query_row(
            "SELECT id FROM conversation WHERE id = ?1 AND status = 'active'",
            [selector],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if exact_id.is_some() {
        return Ok(exact_id);
    }

    let external_id = conn
        .query_row(
            "SELECT id FROM conversation WHERE provider_conversation_id = ?1 AND status = 'active'",
            [selector],
            |row| row.get::<_, String>(0),
        )
        .optional()?;
    if external_id.is_some() {
        return Ok(external_id);
    }

    let mut stmt = conn.prepare(
        r#"
        SELECT id
        FROM conversation
        WHERE status = 'active' AND title = ?1
        ORDER BY updated_at_ms DESC, id ASC
        "#,
    )?;
    let title_matches = stmt
        .query_map([selector], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;

    match title_matches.as_slice() {
        [] => Ok(None),
        [id] => Ok(Some(id.clone())),
        many => anyhow::bail!(
            "conversation selector '{}' is ambiguous: {} exact title matches. Use the internal conversation id from search output or the provider external id. Matching ids: {}",
            selector,
            many.len(),
            many.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
        ),
    }
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
    tx.execute("DELETE FROM conversation_fts", [])
        .context("failed to clear FTS index")?;
    tx.execute("DELETE FROM conversation_fts_nl", [])
        .context("failed to clear natural-language FTS index")?;
    tx.execute("DELETE FROM artifact_lexicon", [])
        .context("failed to clear artifact lexicon")?;

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
        let canonical_path = workspace
            .source_path
            .as_deref()
            .map(normalize_workspace_path)
            .or_else(|| normalize_workspace_display_name(&workspace.display_name))
            .filter(|path| !path.is_empty());
        let display_name = canonical_path
            .as_deref()
            .and_then(prettified_workspace_display_name)
            .unwrap_or_else(|| {
                normalize_workspace_display_name(&workspace.display_name)
                    .unwrap_or_else(|| workspace.display_name.clone())
            });
        let workspace_id = normalized_workspace_id(&workspace, canonical_path.as_deref());
        let workspace_row = ImportedWorkspace {
            id: workspace_id,
            display_name,
            canonical_path,
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
    let branch_parent_provider_id = conversation.branch_parent_id.clone();
    conversation.id = format!(
        "{}:{}:{}",
        provider_key(conversation.provider),
        workspace_id,
        provider_native_id
    );
    conversation.branch_parent_id = branch_parent_provider_id.map(|parent_id| {
        format!(
            "{}:{}:{}",
            provider_key(conversation.provider),
            workspace_id,
            parent_id
        )
    });
}

fn normalized_workspace_id(workspace: &Workspace, canonical_path: Option<&str>) -> String {
    if let Some(source_path) = canonical_path {
        return source_path.to_string();
    }

    normalize_workspace_display_name(&workspace.display_name)
        .unwrap_or_else(|| workspace.display_name.clone())
}

fn normalize_workspace_display_name(display_name: &str) -> Option<String> {
    if display_name == "~" {
        let home_dir = env::var("HOME").ok()?;
        return Some(normalize_workspace_path(&home_dir));
    }

    if let Some(rest) = display_name.strip_prefix("~/") {
        let home_dir = env::var("HOME").ok()?;
        return Some(normalize_workspace_path(&format!("{home_dir}/{rest}")));
    }

    if display_name.starts_with('/') {
        return Some(normalize_workspace_path(display_name));
    }

    None
}

fn normalize_workspace_path(path: &str) -> String {
    let normalized = Path::new(path)
        .components()
        .fold(PathBuf::new(), |mut acc, component| {
            match component {
                Component::CurDir => {}
                other => acc.push(other.as_os_str()),
            }
            acc
        })
        .to_string_lossy()
        .to_string();

    if normalized.is_empty() {
        path.to_string()
    } else {
        normalized
    }
}

fn prettified_workspace_display_name(path: &str) -> Option<String> {
    let home_dir = env::var("HOME").ok()?;
    if path == home_dir {
        return Some("~".to_string());
    }

    path.strip_prefix(&home_dir)
        .map(|suffix| format!("~{}", suffix))
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
    let imported_ids = imported
        .iter()
        .map(|row| row.conversation.id.clone())
        .collect::<BTreeSet<_>>();
    let mut conv_stmt = tx.prepare(
        r#"
        INSERT INTO conversation (
            id, workspace_id, provider, provider_conversation_id, parent_conversation_id, title, preview_text, status,
            created_at_ms, updated_at_ms, last_source_event_at_ms, metadata_json
        ) VALUES (?1, ?2, ?3, ?4, NULL, ?5, ?6, 'active', ?7, ?8, ?9, ?10)
        "#,
    )?;
    let mut parent_stmt = tx.prepare(
        r#"
        UPDATE conversation
        SET parent_conversation_id = ?2
        WHERE id = ?1
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
        let derived_title = row.conversation.display_title();
        let effective_title = (derived_title != row.conversation.id).then_some(derived_title);
        let effective_preview = row
            .conversation
            .preview_line()
            .map(str::to_owned)
            .or_else(|| effective_title.clone());
        let metadata_json = json!({
            "branch_anchor_message_id": row.conversation.branch_anchor_message_id,
        })
        .to_string();
        conv_stmt.execute(params![
            row.conversation.id,
            row.workspace.id,
            provider_key(row.conversation.provider),
            provider_conversation_id,
            effective_title,
            effective_preview,
            row.conversation.created_at,
            row.conversation.updated_at,
            row.conversation.updated_at,
            metadata_json,
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

    for row in imported {
        let Some(parent_id) = row.conversation.branch_parent_id.as_ref() else {
            continue;
        };
        if !imported_ids.contains(parent_id) {
            continue;
        }
        parent_stmt.execute(params![row.conversation.id, parent_id])?;
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
    let mut link_stmt = tx.prepare(
        r#"
        UPDATE conversation_entry
        SET parent_entry_id = ?2, associated_entry_id = ?3
        WHERE id = ?1
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
        r#"
        INSERT INTO conversation_fts (
            entry_id, conversation_id, title_text, preview_text, opening_prompt_text,
            early_user_context_text, artifact_text, search_text
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
        "#,
    )?;
    let mut fts_nl_stmt = tx.prepare(
        r#"
        INSERT INTO conversation_fts_nl (
            entry_id, conversation_id, title_text, preview_text, opening_prompt_text,
            early_user_context_text, artifact_text, search_text
        ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, '', ?7)
        "#,
    )?;
    let mut artifact_doc_freq = BTreeMap::<String, i64>::new();

    for row in imported {
        let entry_ids = assign_entry_ids(&row.conversation);
        let canonical_ids = canonical_provider_entry_ids(&row.conversation, &entry_ids);
        let title_text = row.conversation.display_title();
        let preview_text = row.conversation.preview_line().unwrap_or("").to_string();
        let opening_prompt_text = row.conversation.opening_prompt_text().unwrap_or_default();
        let early_user_context_text = row
            .conversation
            .early_user_context_text()
            .unwrap_or_default();

        for (ordinal, message) in row.conversation.messages.iter().enumerate() {
            let entry_id = &entry_ids[ordinal];
            let source_file_id = row
                .sources
                .first()
                .map(|source| source.source_file_id.clone());
            let search_text = searchable_text(message);
            let artifact_text = search_text
                .as_deref()
                .map(extract_artifact_text)
                .filter(|text| !text.is_empty());
            let participant_label = message.participant.label();

            entry_stmt.execute(params![
                entry_id,
                row.conversation.id,
                message_kind_key(message.kind),
                Option::<String>::None,
                Option::<String>::None,
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
                fts_stmt.execute(params![
                    entry_id,
                    row.conversation.id,
                    title_text,
                    preview_text,
                    opening_prompt_text,
                    early_user_context_text,
                    artifact_text,
                    search_text
                ])?;
                fts_nl_stmt.execute(params![
                    entry_id,
                    row.conversation.id,
                    title_text,
                    preview_text,
                    opening_prompt_text,
                    early_user_context_text,
                    search_text
                ])?;
                if let Some(artifact_text) = artifact_text {
                    let unique_terms = artifact_text
                        .split_whitespace()
                        .map(str::to_string)
                        .collect::<BTreeSet<_>>();
                    for term in unique_terms {
                        *artifact_doc_freq.entry(term).or_insert(0) += 1;
                    }
                }
            }
        }
        for (ordinal, message) in row.conversation.messages.iter().enumerate() {
            if message.parent_id.is_none() && message.associated_id.is_none() {
                continue;
            }
            let parent_entry_id = message
                .parent_id
                .as_ref()
                .and_then(|id| canonical_ids.get(id))
                .cloned();
            let associated_entry_id = message
                .associated_id
                .as_ref()
                .and_then(|id| canonical_ids.get(id))
                .cloned();
            link_stmt.execute(params![
                &entry_ids[ordinal],
                parent_entry_id,
                associated_entry_id
            ])?;
        }
    }

    let mut artifact_stmt = tx.prepare(
        r#"
        INSERT INTO artifact_lexicon (term, doc_freq)
        VALUES (?1, ?2)
        "#,
    )?;
    for (term, doc_freq) in artifact_doc_freq {
        artifact_stmt.execute(params![term, doc_freq])?;
    }

    Ok(())
}

fn assign_entry_ids(conversation: &Conversation) -> Vec<String> {
    let mut seen_raw_ids = BTreeSet::new();

    conversation
        .messages
        .iter()
        .enumerate()
        .map(|(ordinal, message)| match &message.id {
            Some(raw_id) if seen_raw_ids.insert(raw_id.clone()) => {
                format!("{}:{raw_id}", conversation.id)
            }
            _ => format!("{}:{ordinal}", conversation.id),
        })
        .collect()
}

fn canonical_provider_entry_ids(
    conversation: &Conversation,
    assigned_ids: &[String],
) -> BTreeMap<String, String> {
    let mut mapping = BTreeMap::new();

    for (message, assigned_id) in conversation.messages.iter().zip(assigned_ids.iter()) {
        if let Some(raw_id) = &message.id {
            mapping
                .entry(raw_id.clone())
                .or_insert_with(|| assigned_id.clone());
        }
    }

    mapping
}

fn load_conversation_summaries(
    conn: &rusqlite::Connection,
    workspace_id: &str,
) -> Result<Vec<Conversation>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, provider_conversation_id, parent_conversation_id, provider, title, preview_text, created_at_ms, updated_at_ms, metadata_json
        FROM conversation
        WHERE workspace_id = ?1 AND status = 'active'
        ORDER BY updated_at_ms DESC, id ASC
        "#,
    )?;
    let rows = stmt.query_map([workspace_id], |row| {
        let metadata_json: Option<String> = row.get(8)?;
        Ok(Conversation {
            id: row.get(0)?,
            external_id: row.get(1)?,
            branch_parent_id: row.get(2)?,
            branch_anchor_message_id: branch_anchor_message_id_from_metadata(
                metadata_json.as_deref(),
            ),
            title: row.get(4)?,
            preview: row.get(5)?,
            provider: parse_provider_key(&row.get::<_, String>(3)?),
            created_at: row.get::<_, Option<i64>>(6)?.unwrap_or(0),
            updated_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
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
            SELECT id, workspace_id, provider_conversation_id, parent_conversation_id, provider, title, preview_text,
                   created_at_ms, updated_at_ms, metadata_json
            FROM conversation
            WHERE id = ?1 AND status = 'active'
            "#,
            [conversation_id],
            |row| {
                Ok(Conversation {
                    id: row.get(0)?,
                    external_id: row.get(2)?,
                    branch_parent_id: row.get(3)?,
                    branch_anchor_message_id: branch_anchor_message_id_from_metadata(
                        row.get::<_, Option<String>>(9)?.as_deref(),
                    ),
                    title: row.get(5)?,
                    preview: row.get(6)?,
                    provider: parse_provider_key(&row.get::<_, String>(4)?),
                    created_at: row.get::<_, Option<i64>>(7)?.unwrap_or(0),
                    updated_at: row.get::<_, Option<i64>>(8)?.unwrap_or(0),
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

fn branch_anchor_message_id_from_metadata(metadata_json: Option<&str>) -> Option<String> {
    let metadata_json = metadata_json?;
    let value = serde_json::from_str::<serde_json::Value>(metadata_json).ok()?;
    value
        .get("branch_anchor_message_id")
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
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

fn extract_artifact_text(text: &str) -> String {
    let mut tokens = Vec::new();
    let mut seen = BTreeSet::new();

    for captures in BACKTICK_RE.captures_iter(text) {
        if let Some(fragment) = captures.get(1).map(|m| m.as_str()) {
            add_artifact_token(fragment, &mut seen, &mut tokens);
        }
    }
    for captures in ABSOLUTE_PATH_RE.captures_iter(text) {
        if let Some(path) = captures.name("path").map(|m| m.as_str()) {
            add_artifact_token(path, &mut seen, &mut tokens);
        }
    }
    for captures in FLAG_RE.captures_iter(text) {
        if let Some(flag) = captures.name("flag").map(|m| m.as_str()) {
            add_artifact_token(flag, &mut seen, &mut tokens);
        }
    }
    for captures in SLASH_COMMAND_RE.captures_iter(text) {
        if let Some(cmd) = captures.name("cmd").map(|m| m.as_str()) {
            add_artifact_token(cmd, &mut seen, &mut tokens);
        }
    }
    for captures in DOTTED_IDENTIFIER_RE.captures_iter(text) {
        if let Some(ident) = captures.name("ident").map(|m| m.as_str()) {
            add_artifact_token(ident, &mut seen, &mut tokens);
        }
    }
    for captures in CODELIKE_TOKEN_RE.captures_iter(text) {
        if let Some(token) = captures.name("token").map(|m| m.as_str()) {
            add_artifact_token(token, &mut seen, &mut tokens);
        }
    }
    for captures in WORD_RE.captures_iter(text) {
        if let Some(token) = captures.name("token").map(|m| m.as_str()) {
            if looks_camel_case_identifier(token) {
                add_artifact_token(token, &mut seen, &mut tokens);
            }
        }
    }

    tokens.join(" ")
}

fn add_artifact_token(token: &str, seen: &mut BTreeSet<String>, tokens: &mut Vec<String>) {
    let trimmed = token
        .trim()
        .trim_matches(|ch: char| matches!(ch, '"' | '\'' | '(' | ')' | '[' | ']'));
    if trimmed.is_empty() {
        return;
    }

    push_artifact_part(trimmed, seen, tokens);
    for part in trimmed.split(|ch: char| matches!(ch, '.' | '/' | '\\' | '_' | '-')) {
        push_artifact_part(part, seen, tokens);
        for camel_part in split_camel_case_parts(part) {
            push_artifact_part(camel_part, seen, tokens);
        }
    }
}

fn push_artifact_part(part: &str, seen: &mut BTreeSet<String>, tokens: &mut Vec<String>) {
    let part = part.trim();
    if part.len() < 2 || part.chars().all(|ch| ch.is_ascii_digit()) {
        return;
    }
    let normalized = part.to_ascii_lowercase();
    if seen.insert(normalized.clone()) {
        tokens.push(normalized);
    }
}

fn looks_camel_case_identifier(token: &str) -> bool {
    if token.len() < 6 {
        return false;
    }
    let chars = token.chars().collect::<Vec<_>>();
    let has_upper = chars.iter().any(|ch| ch.is_ascii_uppercase());
    let has_lower = chars.iter().any(|ch| ch.is_ascii_lowercase());
    has_upper
        && has_lower
        && chars
            .windows(2)
            .any(|pair| pair[0].is_ascii_lowercase() && pair[1].is_ascii_uppercase())
}

fn split_camel_case_parts(token: &str) -> Vec<&str> {
    if !looks_camel_case_identifier(token) {
        return Vec::new();
    }

    let mut parts = Vec::new();
    let mut start = 0;
    let chars = token.char_indices().collect::<Vec<_>>();
    for window in chars.windows(2) {
        let (_, left) = window[0];
        let (right_idx, right) = window[1];
        if left.is_ascii_lowercase() && right.is_ascii_uppercase() {
            parts.push(&token[start..right_idx]);
            start = right_idx;
        }
    }
    parts.push(&token[start..]);
    parts
}

fn choose_search_snippet(title: &str, preview_text: Option<&str>, matched_snippet: &str) -> String {
    let matched_snippet = matched_snippet.trim();
    if matched_snippet_has_highlight(matched_snippet) {
        return matched_snippet.to_string();
    }

    if let Some(preview_text) = preview_text.map(str::trim).filter(|text| !text.is_empty()) {
        if preview_text != title {
            return preview_text.to_string();
        }
    }

    if !matched_snippet.is_empty() {
        return matched_snippet.to_string();
    }

    preview_text
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .unwrap_or(title)
        .to_string()
}

fn matched_snippet_has_highlight(snippet: &str) -> bool {
    if !(snippet.contains('[') && snippet.contains(']')) {
        return false;
    }

    let trimmed = snippet.trim();
    !(trimmed.starts_with('[')
        && trimmed.ends_with(']')
        && trimmed.matches('[').count() == 1
        && trimmed.matches(']').count() == 1)
}

#[cfg(test)]
fn tokenize_search_query(query: &str) -> Vec<String> {
    tokenize_exact_search_query_with_workspace(query, None)
}

fn tokenize_exact_search_query_with_workspace(
    query: &str,
    workspace_hint: Option<&WorkspaceSearchHint>,
) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = BTreeSet::new();
    let workspace_match_tokens = workspace_hint
        .map(|hint| {
            hint.matches
                .iter()
                .flat_map(|value| {
                    std::iter::once(value.to_ascii_lowercase())
                        .chain(
                            value
                        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                        .filter(|part| !part.is_empty())
                        .map(|part| part.to_ascii_lowercase())
                        )
                        .collect::<Vec<_>>()
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    for raw in query
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
    {
        let normalized = raw.trim().to_ascii_lowercase();
        if QUERY_STOPWORDS.contains(&normalized.as_str()) {
            continue;
        }
        if workspace_match_tokens.contains(&normalized) {
            continue;
        }
        push_query_token(&normalized, &mut seen, &mut tokens);
        if normalized.len() > 4 && normalized.ends_with('s') {
            push_query_token(&normalized[..normalized.len() - 1], &mut seen, &mut tokens);
        }
    }

    let artifacts = extract_artifact_text(query);
    for artifact in artifacts.split_whitespace() {
        if workspace_match_tokens.contains(artifact) {
            continue;
        }
        push_query_token(artifact, &mut seen, &mut tokens);
        if artifact.len() > 4 && artifact.ends_with('s') {
            push_query_token(&artifact[..artifact.len() - 1], &mut seen, &mut tokens);
        }
    }

    tokens
}

fn tokenize_nl_search_query_with_workspace(
    query: &str,
    workspace_hint: Option<&WorkspaceSearchHint>,
) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut seen = BTreeSet::new();
    let workspace_match_tokens = workspace_hint
        .map(|hint| {
            hint.matches
                .iter()
                .flat_map(|value| {
                    std::iter::once(value.to_ascii_lowercase())
                        .chain(
                            value
                                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                                .filter(|part| !part.is_empty())
                                .map(|part| part.to_ascii_lowercase()),
                        )
                        .collect::<Vec<_>>()
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    for raw in query
        .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
        .filter(|token| !token.is_empty())
    {
        let normalized = raw.trim().to_ascii_lowercase();
        if QUERY_STOPWORDS.contains(&normalized.as_str()) {
            continue;
        }
        if workspace_match_tokens.contains(&normalized) {
            continue;
        }
        push_query_token(&normalized, &mut seen, &mut tokens);
        if normalized.len() > 4 && normalized.ends_with('s') {
            push_query_token(&normalized[..normalized.len() - 1], &mut seen, &mut tokens);
        }
    }

    tokens
}

fn push_query_token(token: &str, seen: &mut BTreeSet<String>, tokens: &mut Vec<String>) {
    let normalized = token.trim().to_ascii_lowercase();
    if normalized.len() < 2 {
        return;
    }
    if seen.insert(normalized.clone()) {
        tokens.push(normalized);
    }
}

fn build_fts_search_tiers(tokens: &[String]) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let quoted_tokens = tokens
        .iter()
        .map(|token| format!("\"{token}\""))
        .collect::<Vec<_>>();
    let mut tiers = Vec::new();

    if tokens.len() > 1 {
        tiers.push(format!("\"{}\"", tokens.join(" ")));
        tiers.push(format!("NEAR({}, 6)", quoted_tokens.join(" ")));
    }

    tiers.push(quoted_tokens.join(" "));

    if tokens.len() > 1 {
        tiers.push(quoted_tokens.join(" OR "));
    }

    tiers
}

fn build_nl_fts_search_tiers(tokens: &[String]) -> Vec<String> {
    if tokens.is_empty() {
        return Vec::new();
    }

    let quoted_tokens = tokens
        .iter()
        .map(|token| format!("\"{token}\""))
        .collect::<Vec<_>>();
    let phrase = format!("\"{}\"", tokens.join(" "));
    let focused_fields = ["title_text", "opening_prompt_text", "early_user_context_text"];
    let focused_phrase = focused_fields
        .iter()
        .map(|field| format!("{field}:{phrase}"))
        .collect::<Vec<_>>()
        .join(" OR ");
    let focused_and = focused_fields
        .iter()
        .map(|field| format!("{field}:{}", quoted_tokens.join(" ")))
        .collect::<Vec<_>>()
        .join(" OR ");

    let mut tiers = Vec::new();

    if tokens.len() > 1 {
        tiers.push(focused_phrase);
        tiers.push(focused_and);
        tiers.push(phrase.clone());
        tiers.push(format!("NEAR({}, 6)", quoted_tokens.join(" ")));
    } else {
        tiers.push(
            focused_fields
                .iter()
                .map(|field| format!("{field}:{}", quoted_tokens[0]))
                .collect::<Vec<_>>()
                .join(" OR "),
        );
    }

    if tokens.len() >= 2 {
        let bigrams = tokens
            .windows(2)
            .map(|window| format!("\"{} {}\"", window[0], window[1]))
            .collect::<Vec<_>>();
        if !bigrams.is_empty() {
            let focused_bigrams = focused_fields
                .iter()
                .map(|field| {
                    bigrams
                        .iter()
                        .map(|bigram| format!("{field}:{bigram}"))
                        .collect::<Vec<_>>()
                        .join(" OR ")
                })
                .collect::<Vec<_>>()
                .join(" OR ");
            tiers.push(focused_bigrams);
        }
    }
    tiers.push(quoted_tokens.join(" "));
    if tokens.len() > 1 {
        tiers.push(quoted_tokens.join(" OR "));
    }
    tiers
}

fn build_exact_tiers_with_correction(
    mut exact_tiers: Vec<String>,
    corrected_artifact_terms: Option<&[String]>,
) -> Vec<String> {
    let Some(corrected_artifact_terms) = corrected_artifact_terms else {
        return exact_tiers;
    };
    if corrected_artifact_terms.is_empty() {
        return exact_tiers;
    }

    let corrected_tiers = corrected_artifact_terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>();
    let insert_at = exact_tiers.len().saturating_sub(1);
    for (offset, tier) in corrected_tiers.into_iter().enumerate() {
        exact_tiers.insert(insert_at + offset, tier);
    }
    exact_tiers
}

fn build_exact_tiers(
    exact_tokens: &[String],
    artifact_terms: &[String],
    corrected_artifact_terms: Option<&[String]>,
) -> Vec<String> {
    let mut tiers = Vec::new();

    if let Some(corrected_artifact_terms) = corrected_artifact_terms {
        if !corrected_artifact_terms.is_empty() {
            tiers.extend(build_artifact_field_tiers(
                "artifact_text",
                corrected_artifact_terms,
            ));
        }
    }
    if !artifact_terms.is_empty() {
        tiers.extend(build_artifact_field_tiers("artifact_text", artifact_terms));
    }

    let exact_tiers = build_fts_search_tiers(exact_tokens);
    tiers.extend(build_exact_tiers_with_correction(
        exact_tiers,
        corrected_artifact_terms,
    ));
    dedupe_preserving_order(tiers)
}

fn build_artifact_field_tiers(field: &str, terms: &[String]) -> Vec<String> {
    if terms.is_empty() {
        return Vec::new();
    }

    let quoted_terms = terms
        .iter()
        .map(|term| format!("\"{term}\""))
        .collect::<Vec<_>>();
    let mut tiers = Vec::new();

    if terms.len() > 1 {
        tiers.push(format!("{field}:\"{}\"", terms.join(" ")));
        tiers.push(format!("{field}:NEAR({}, 6)", quoted_terms.join(" ")));
    }

    tiers.push(format!("{field}:{}", quoted_terms.join(" ")));

    if terms.len() > 1 {
        tiers.push(format!("{field}:{}", quoted_terms.join(" OR ")));
    }

    tiers
}

fn dedupe_preserving_order(items: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for item in items {
        if seen.insert(item.clone()) {
            deduped.push(item);
        }
    }
    deduped
}

fn execute_fts_search_tier(
    conn: &rusqlite::Connection,
    fts_query: &str,
    search_context: &SearchContext,
    surface: SearchSurface,
    limit: usize,
) -> Result<Vec<SearchResult>> {
    let (table_name, title_weight, preview_weight, opening_prompt_weight, early_context_weight, artifact_weight, body_weight) =
        match surface {
            SearchSurface::Exact => (
                "conversation_fts",
                TITLE_WEIGHT,
                PREVIEW_WEIGHT,
                OPENING_PROMPT_WEIGHT,
                EARLY_CONTEXT_WEIGHT,
                ARTIFACT_WEIGHT,
                BODY_WEIGHT,
            ),
            SearchSurface::NaturalLanguage => (
                "conversation_fts_nl",
                NL_TITLE_WEIGHT,
                NL_PREVIEW_WEIGHT,
                NL_OPENING_PROMPT_WEIGHT,
                NL_EARLY_CONTEXT_WEIGHT,
                NL_ARTIFACT_WEIGHT,
                NL_BODY_WEIGHT,
            ),
    };
    let sql = format!(
        r#"
        SELECT
            c.id,
            c.provider_conversation_id,
            COALESCE(w.display_name, ''),
            c.provider,
            COALESCE(c.title, c.preview_text, c.id),
            c.preview_text,
            snippet({table_name}, 7, '[', ']', ' … ', 18),
            {table_name}.entry_id,
            COALESCE(c.updated_at_ms, 0),
            c.parent_conversation_id,
            parent.title
        FROM {table_name}
        JOIN conversation c ON c.id = {table_name}.conversation_id
        LEFT JOIN workspace w ON w.id = c.workspace_id
        LEFT JOIN conversation parent ON parent.id = c.parent_conversation_id
        WHERE {table_name} MATCH ?1
          AND c.status = 'active'
          AND (?11 IS NULL OR c.workspace_id = ?11)
        ORDER BY
            CASE
                WHEN ?2 IS NOT NULL AND c.workspace_id = ?2 THEN 0
                ELSE 1
            END,
            CASE
                WHEN ?3 IS NOT NULL AND COALESCE(c.updated_at_ms, 0) >= ?3 THEN 0
                ELSE 1
            END,
            bm25({table_name}, ?4, ?5, ?6, ?7, ?8, ?9),
            c.updated_at_ms DESC
        LIMIT ?10
        "#
    );
    let mut stmt = conn.prepare(&sql)?;

    let rows = stmt.query_map(
        params![
            fts_query,
            search_context.workspace_id_hint,
            search_context.recent_cutoff_ms,
            title_weight,
            preview_weight,
            opening_prompt_weight,
            early_context_weight,
            artifact_weight,
            body_weight,
            limit as i64,
            search_context.workspace_id_filter,
        ],
        |row| {
            let title: String = row.get(4)?;
            let preview_text: Option<String> = row.get(5)?;
            let matched_snippet: Option<String> = row.get(6)?;
            Ok(SearchResult {
                conversation_id: row.get(0)?,
                external_conversation_id: row.get(1)?,
                workspace: row.get(2)?,
                provider: row.get(3)?,
                title: title.clone(),
                snippet: choose_search_snippet(
                    &title,
                    preview_text.as_deref(),
                    matched_snippet.as_deref().unwrap_or_default(),
                ),
                matched_entry_id: row.get(7)?,
                updated_at_ms: row.get(8)?,
                is_branch: row.get::<_, Option<String>>(9)?.is_some(),
                branch_parent_conversation_id: row.get(9)?,
                branch_parent_title: row.get(10)?,
            })
        },
    )?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to execute FTS search tier")
}

fn source_file_id(provider: ProviderKind, path: &str) -> String {
    format!("{}:{path}", provider_key(provider))
}

fn build_search_context(
    conn: &rusqlite::Connection,
    query: &str,
) -> Result<SearchContext> {
    let workspace_hints = load_workspace_search_hints(conn)?;
    let workspace_hint = detect_workspace_hint(query, &workspace_hints);
    let exact_tokens = tokenize_exact_search_query_with_workspace(query, workspace_hint);
    let nl_tokens = tokenize_nl_search_query_with_workspace(query, workspace_hint);
    let artifact_terms = extract_query_artifact_terms_with_workspace(query, workspace_hint);
    let prefers_exact_surface = query_prefers_exact_surface(query);
    let corrected_artifact_terms = if prefers_exact_surface {
        maybe_correct_artifact_terms(conn, query, workspace_hint)?
    } else {
        None
    };
    Ok(SearchContext {
        exact_tokens,
        nl_tokens,
        artifact_terms,
        corrected_artifact_terms,
        workspace_id_hint: workspace_hint.map(|hint| hint.workspace_id.clone()),
        workspace_id_filter: workspace_hint.map(|hint| hint.workspace_id.clone()),
        recent_cutoff_ms: detect_recent_cutoff_ms(query),
        prefers_exact_surface,
    })
}

fn query_prefers_exact_surface(query: &str) -> bool {
    let extracted_artifacts = extract_artifact_text(query);
    if !extracted_artifacts.is_empty() {
        return true;
    }

    query.chars().any(|ch| matches!(ch, '/' | '.' | '_' | '-' | '`'))
}

fn maybe_correct_artifact_terms(
    conn: &rusqlite::Connection,
    query: &str,
    workspace_hint: Option<&WorkspaceSearchHint>,
) -> Result<Option<Vec<String>>> {
    let artifact_terms = extract_query_artifact_terms_with_workspace(query, workspace_hint);
    if artifact_terms.is_empty() {
        return Ok(None);
    }

    let lexicon = load_artifact_lexicon(conn)?;
    let terms = lexicon.keys().cloned().collect::<Vec<_>>();
    if terms.is_empty() {
        return Ok(None);
    }

    let mut corrected = artifact_terms.clone();
    let mut changed = false;
    for token in &mut corrected {
        if lexicon.contains_key(token) {
            continue;
        }
        if let Some(replacement) = closest_artifact_term(token, &terms) {
            *token = replacement;
            changed = true;
        }
    }

    if changed { Ok(Some(corrected)) } else { Ok(None) }
}

fn extract_query_artifact_terms_with_workspace(
    query: &str,
    workspace_hint: Option<&WorkspaceSearchHint>,
) -> Vec<String> {
    let mut terms = Vec::new();
    let mut seen = BTreeSet::new();
    let workspace_match_tokens = workspace_hint
        .map(|hint| {
            hint.matches
                .iter()
                .flat_map(|value| {
                    std::iter::once(value.to_ascii_lowercase())
                        .chain(
                            value
                                .split(|ch: char| !(ch.is_alphanumeric() || ch == '_'))
                                .filter(|part| !part.is_empty())
                                .map(|part| part.to_ascii_lowercase()),
                        )
                        .collect::<Vec<_>>()
                })
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();

    for captures in BACKTICK_RE.captures_iter(query) {
        if let Some(value) = captures.get(1).map(|m| m.as_str()) {
            push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
        }
    }
    for captures in ABSOLUTE_PATH_RE.captures_iter(query) {
        if let Some(value) = captures.name("path").map(|m| m.as_str()) {
            push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
        }
    }
    for captures in FLAG_RE.captures_iter(query) {
        if let Some(value) = captures.name("flag").map(|m| m.as_str()) {
            push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
        }
    }
    for captures in SLASH_COMMAND_RE.captures_iter(query) {
        if let Some(value) = captures.name("cmd").map(|m| m.as_str()) {
            push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
        }
    }
    for captures in DOTTED_IDENTIFIER_RE.captures_iter(query) {
        if let Some(value) = captures.name("ident").map(|m| m.as_str()) {
            push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
        }
    }
    for captures in CODELIKE_TOKEN_RE.captures_iter(query) {
        if let Some(value) = captures.name("token").map(|m| m.as_str()) {
            push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
        }
    }
    for captures in WORD_RE.captures_iter(query) {
        if let Some(value) = captures.name("token").map(|m| m.as_str()) {
            if looks_camel_case_identifier(value) {
                push_query_artifact_term(value, &workspace_match_tokens, &mut seen, &mut terms);
            }
        }
    }

    terms
}

fn push_query_artifact_term(
    term: &str,
    workspace_match_tokens: &BTreeSet<String>,
    seen: &mut BTreeSet<String>,
    terms: &mut Vec<String>,
) {
    let normalized = term.trim().to_ascii_lowercase();
    if normalized.len() < 2 {
        return;
    }
    if workspace_match_tokens.contains(&normalized) {
        return;
    }
    if seen.insert(normalized.clone()) {
        terms.push(normalized);
    }
}

fn load_artifact_lexicon(conn: &rusqlite::Connection) -> Result<BTreeMap<String, i64>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT term, doc_freq
        FROM artifact_lexicon
        ORDER BY term ASC
        "#,
    )?;
    let rows = stmt.query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)))?;
    rows.collect::<rusqlite::Result<BTreeMap<_, _>>>()
        .context("failed to load artifact lexicon")
}

fn closest_artifact_term(token: &str, lexicon_terms: &[String]) -> Option<String> {
    let max_distance = if token.len() >= 12 {
        2
    } else if token.len() >= 5 {
        1
    } else {
        0
    };
    if max_distance == 0 {
        return None;
    }

    let mut best: Option<(&str, usize)> = None;
    let mut ambiguous = false;
    for candidate in lexicon_terms {
        if candidate.len().abs_diff(token.len()) > max_distance {
            continue;
        }
        let distance = levenshtein_distance_bounded(token, candidate, max_distance);
        if let Some(distance) = distance {
            match best {
                None => {
                    best = Some((candidate.as_str(), distance));
                    ambiguous = false;
                }
                Some((_, best_distance)) if distance < best_distance => {
                    best = Some((candidate.as_str(), distance));
                    ambiguous = false;
                }
                Some((_, best_distance)) if distance == best_distance => {
                    ambiguous = true;
                }
                _ => {}
            }
        }
    }

    match (best, ambiguous) {
        (Some((candidate, _)), false) => Some(candidate.to_string()),
        _ => None,
    }
}

fn levenshtein_distance_bounded(left: &str, right: &str, max_distance: usize) -> Option<usize> {
    if left == right {
        return Some(0);
    }
    let left_chars = left.chars().collect::<Vec<_>>();
    let right_chars = right.chars().collect::<Vec<_>>();
    if left_chars.len().abs_diff(right_chars.len()) > max_distance {
        return None;
    }

    let mut prev = (0..=right_chars.len()).collect::<Vec<_>>();
    let mut curr = vec![0; right_chars.len() + 1];

    for (i, left_char) in left_chars.iter().enumerate() {
        curr[0] = i + 1;
        let mut row_min = curr[0];
        for (j, right_char) in right_chars.iter().enumerate() {
            let cost = usize::from(left_char != right_char);
            curr[j + 1] = (prev[j + 1] + 1)
                .min(curr[j] + 1)
                .min(prev[j] + cost);
            row_min = row_min.min(curr[j + 1]);
        }
        if row_min > max_distance {
            return None;
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    (prev[right_chars.len()] <= max_distance).then_some(prev[right_chars.len()])
}

fn load_workspace_search_hints(conn: &rusqlite::Connection) -> Result<Vec<WorkspaceSearchHint>> {
    let mut stmt = conn.prepare(
        r#"
        SELECT id, display_name, canonical_path
        FROM workspace
        WHERE status = 'active'
        "#,
    )?;
    let rows = stmt.query_map([], |row| {
        let id: String = row.get(0)?;
        let display_name: String = row.get(1)?;
        let canonical_path: Option<String> = row.get(2)?;
        Ok(WorkspaceSearchHint {
            workspace_id: id,
            matches: workspace_match_candidates(&display_name, canonical_path.as_deref()),
        })
    })?;

    rows.collect::<rusqlite::Result<Vec<_>>>()
        .context("failed to load workspace search hints")
}

fn workspace_match_candidates(display_name: &str, canonical_path: Option<&str>) -> Vec<String> {
    let mut values = BTreeSet::new();
    if let Some(base) = workspace_basename(display_name) {
        values.insert(base);
    }
    if let Some(path) = canonical_path {
        if let Some(base) = workspace_basename(path) {
            values.insert(base);
        }
    }
    values.into_iter().collect()
}

fn workspace_basename(value: &str) -> Option<String> {
    let trimmed = value.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "~" {
        return None;
    }
    let candidate = trimmed.rsplit('/').next().unwrap_or(trimmed).trim();
    if candidate.is_empty() || candidate == "~" {
        return None;
    }
    Some(candidate.to_ascii_lowercase())
}

fn detect_workspace_hint<'a>(
    query: &str,
    workspace_hints: &'a [WorkspaceSearchHint],
) -> Option<&'a WorkspaceSearchHint> {
    let lower_query = query.to_ascii_lowercase();
    workspace_hints
        .iter()
        .filter(|hint| {
            hint.matches
                .iter()
                .any(|candidate| lower_query.contains(candidate))
        })
        .max_by_key(|hint| {
            hint.matches
                .iter()
                .map(|candidate| candidate.len())
                .max()
                .unwrap_or(0)
        })
}

fn detect_recent_cutoff_ms(query: &str) -> Option<i64> {
    let lower_query = query.to_ascii_lowercase();
    let days = if lower_query.contains("today") {
        Some(1)
    } else if lower_query.contains("yesterday") {
        Some(2)
    } else if lower_query.contains("couple days ago")
        || lower_query.contains("a couple days ago")
        || lower_query.contains("few days ago")
    {
        Some(7)
    } else if lower_query.contains("this week") {
        Some(7)
    } else if lower_query.contains("last week") {
        Some(14)
    } else {
        None
    }?;

    let now_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_millis() as i64;
    Some(now_ms - days * 24 * 60 * 60 * 1000)
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

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn conversation_with_ids(ids: &[Option<&str>]) -> Conversation {
        Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: Vec::new(),
            messages: ids
                .iter()
                .map(|id| Message {
                    id: id.map(|value| value.to_string()),
                    kind: MessageKind::AssistantMessage,
                    participant: Participant::Assistant {
                        provider: ProviderKind::ClaudeCode,
                    },
                    content: "x".into(),
                    timestamp: None,
                    parent_id: None,
                    associated_id: None,
                    depth: 0,
                })
                .collect(),
            is_hydrated: true,
            load_ref: None,
        }
    }

    #[test]
    fn assign_entry_ids_falls_back_on_duplicates() {
        let conversation = conversation_with_ids(&[Some("dup"), Some("dup"), None]);
        let ids = assign_entry_ids(&conversation);

        assert_eq!(ids, vec!["conv-1:dup", "conv-1:1", "conv-1:2"]);
    }

    #[test]
    fn canonical_provider_entry_ids_prefers_first_occurrence() {
        let conversation = conversation_with_ids(&[Some("dup"), Some("dup"), Some("other")]);
        let assigned = assign_entry_ids(&conversation);
        let canonical = canonical_provider_entry_ids(&conversation, &assigned);

        assert_eq!(canonical.get("dup").map(String::as_str), Some("conv-1:dup"));
        assert_eq!(
            canonical.get("other").map(String::as_str),
            Some("conv-1:other")
        );
    }

    #[test]
    fn normalize_workspace_path_collapses_home_trailing_slash() {
        assert_eq!(
            normalize_workspace_path("/Users/gaborkerekes//"),
            "/Users/gaborkerekes"
        );
    }

    #[test]
    fn prettified_workspace_display_name_uses_tilde_for_home_dir() {
        let home_dir = env::var("HOME").unwrap_or_else(|_| "/Users/gaborkerekes".into());
        let display = prettified_workspace_display_name(&home_dir);

        assert_eq!(display.as_deref(), Some("~"));
    }

    #[test]
    fn flatten_workspaces_derives_home_canonical_path_from_tilde_display_name() {
        let workspace = Workspace {
            id: "-".into(),
            display_name: "~".into(),
            source_path: None,
            updated_at: 123,
            conversations: vec![Conversation {
                id: "conv".into(),
                external_id: None,
                branch_parent_id: None,
                branch_anchor_message_id: None,
                title: Some("Home".into()),
                preview: None,
                provider: ProviderKind::ClaudeCode,
                created_at: 123,
                updated_at: 123,
                segments: Vec::new(),
                messages: Vec::new(),
                is_hydrated: false,
                load_ref: None,
            }],
        };

        let flattened = flatten_workspaces(vec![workspace]);
        let home_dir = env::var("HOME").unwrap_or_else(|_| "/Users/gaborkerekes".into());

        assert_eq!(flattened.len(), 1);
        assert_eq!(
            flattened[0].workspace.canonical_path.as_deref(),
            Some(home_dir.as_str())
        );
        assert_eq!(flattened[0].workspace.display_name, "~");
        assert_eq!(flattened[0].workspace.id, home_dir);
    }

    #[test]
    fn choose_search_snippet_prefers_preview_over_low_signal_match_snippet() {
        let snippet = choose_search_snippet(
            "lightdash-weekly-analytics-dashboard",
            Some("We need programmatic access to a Lightdash instance running on Cloud Run behind IAP."),
            "[Request interrupted by user for tool use]",
        );

        assert_eq!(
            snippet,
            "We need programmatic access to a Lightdash instance running on Cloud Run behind IAP."
        );
    }

    #[test]
    fn choose_search_snippet_falls_back_to_match_when_preview_equals_title() {
        let snippet = choose_search_snippet(
            "which lightdash",
            Some("which lightdash"),
            "matched assistant line",
        );

        assert_eq!(snippet, "matched assistant line");
    }

    #[test]
    fn choose_search_snippet_prefers_highlighted_match_over_preview() {
        let snippet = choose_search_snippet(
            "effect-http-api-spike",
            Some("the implementation of the /chat endpoint is significantly more complex than i would like it to be."),
            "added [lightdash] deployment wiring",
        );

        assert_eq!(snippet, "added [lightdash] deployment wiring");
    }

    #[test]
    fn build_fts_search_tiers_includes_phrase_near_and_or_for_multi_token_queries() {
        let tiers = build_fts_search_tiers(&["working".into(), "memory".into()]);

        assert_eq!(
            tiers,
            vec![
                "\"working memory\"",
                "NEAR(\"working\" \"memory\", 6)",
                "\"working\" \"memory\"",
                "\"working\" OR \"memory\"",
            ]
        );
    }

    #[test]
    fn build_fts_search_tiers_uses_single_and_query_for_one_token() {
        let tiers = build_fts_search_tiers(&["lightdash".into()]);

        assert_eq!(tiers, vec!["\"lightdash\""]);
    }

    #[test]
    fn build_nl_fts_search_tiers_adds_bigram_phrase_expansion() {
        let tiers = build_nl_fts_search_tiers(&[
            "persistent".into(),
            "codex".into(),
            "sessions".into(),
        ]);

        assert_eq!(
            tiers,
            vec![
                "title_text:\"persistent codex sessions\" OR opening_prompt_text:\"persistent codex sessions\" OR early_user_context_text:\"persistent codex sessions\"",
                "title_text:\"persistent\" \"codex\" \"sessions\" OR opening_prompt_text:\"persistent\" \"codex\" \"sessions\" OR early_user_context_text:\"persistent\" \"codex\" \"sessions\"",
                "\"persistent codex sessions\"",
                "NEAR(\"persistent\" \"codex\" \"sessions\", 6)",
                "title_text:\"persistent codex\" OR title_text:\"codex sessions\" OR opening_prompt_text:\"persistent codex\" OR opening_prompt_text:\"codex sessions\" OR early_user_context_text:\"persistent codex\" OR early_user_context_text:\"codex sessions\"",
                "\"persistent\" \"codex\" \"sessions\"",
                "\"persistent\" OR \"codex\" OR \"sessions\"",
            ]
        );
    }

    #[test]
    fn query_prefers_exact_surface_for_artifact_like_queries() {
        assert!(query_prefers_exact_surface("serviceusage.services.use"));
        assert!(query_prefers_exact_surface("codex exec --full-auto"));
        assert!(!query_prefers_exact_surface("persistent Codex sessions"));
    }

    #[test]
    fn build_exact_tiers_with_correction_inserts_before_broad_fallback() {
        let tiers = build_exact_tiers_with_correction(
            build_fts_search_tiers(&["servicusage.services.use".into()]),
            Some(&["serviceusage.services.use".into()]),
        );

        assert_eq!(
            tiers,
            vec![
                "\"serviceusage.services.use\"",
                "\"servicusage.services.use\"",
            ]
        );
    }

    #[test]
    fn build_exact_tiers_prioritizes_artifact_field_before_general_exact_matching() {
        let tiers = build_exact_tiers(
            &["servicusage.services.use".into(), "serviceusage".into()],
            &["servicusage.services.use".into()],
            Some(&["serviceusage.services.use".into()]),
        );

        assert_eq!(
            tiers,
            vec![
                "artifact_text:\"serviceusage.services.use\"",
                "artifact_text:\"servicusage.services.use\"",
                "\"servicusage.services.use serviceusage\"",
                "NEAR(\"servicusage.services.use\" \"serviceusage\", 6)",
                "\"servicusage.services.use\" \"serviceusage\"",
                "\"serviceusage.services.use\"",
                "\"servicusage.services.use\" OR \"serviceusage\"",
            ]
        );
    }

    #[test]
    fn closest_artifact_term_prefers_unique_bounded_match() {
        let terms = vec![
            "serviceusage.services.use".to_string(),
            "serviceusage.services.list".to_string(),
        ];

        let corrected = closest_artifact_term("servicusage.services.use", &terms);

        assert_eq!(corrected.as_deref(), Some("serviceusage.services.use"));
    }

    #[test]
    fn levenshtein_distance_bounded_rejects_large_distance() {
        assert_eq!(
            levenshtein_distance_bounded("lightdasg", "lightdash", 1),
            Some(1)
        );
        assert_eq!(
            levenshtein_distance_bounded("lightdasg", "serviceusage", 1),
            None
        );
    }

    #[test]
    fn extract_artifact_text_keeps_raw_and_split_forms() {
        let artifacts = extract_artifact_text(
            "need `serviceusage.services.use` on /Users/gaborkerekes/foo-bar and flag --full-auto with VZBridgedNetworkDeviceAttachment",
        );

        assert!(artifacts.contains("serviceusage.services.use"));
        assert!(artifacts.contains("serviceusage"));
        assert!(artifacts.contains("services"));
        assert!(artifacts.contains("use"));
        assert!(artifacts.contains("/users/gaborkerekes/foo-bar"));
        assert!(artifacts.contains("foo-bar"));
        assert!(artifacts.contains("foo"));
        assert!(artifacts.contains("bar"));
        assert!(artifacts.contains("--full-auto"));
        assert!(artifacts.contains("full"));
        assert!(artifacts.contains("auto"));
        assert!(artifacts.contains("vzbridgednetworkdeviceattachment"));
        assert!(artifacts.contains("bridged"));
        assert!(artifacts.contains("network"));
        assert!(artifacts.contains("device"));
        assert!(artifacts.contains("attachment"));
    }

    #[test]
    fn tokenize_search_query_adds_simple_singular_forms() {
        let tokens = tokenize_search_query("persistent Codex sessions");

        assert!(tokens.contains(&"persistent".to_string()));
        assert!(tokens.contains(&"codex".to_string()));
        assert!(tokens.contains(&"sessions".to_string()));
        assert!(tokens.contains(&"session".to_string()));
    }

    #[test]
    fn tokenize_search_query_drops_workspace_hint_terms() {
        let workspace_hint = WorkspaceSearchHint {
            workspace_id: "sibyl".into(),
            matches: vec!["sibyl-memory-mvp".into()],
        };

        let exact = tokenize_exact_search_query_with_workspace(
            "servicusage.services.use in sibyl-memory-mvp",
            Some(&workspace_hint),
        );
        let nl = tokenize_nl_search_query_with_workspace(
            "a couple days ago we discussed lightdash in sibyl-memory-mvp",
            Some(&workspace_hint),
        );

        assert!(!exact.contains(&"sibyl".to_string()));
        assert!(!exact.contains(&"memory".to_string()));
        assert!(!exact.contains(&"mvp".to_string()));
        assert!(!nl.contains(&"sibyl".to_string()));
        assert!(!nl.contains(&"memory".to_string()));
        assert!(!nl.contains(&"mvp".to_string()));
    }

    #[test]
    fn extract_query_artifact_terms_drops_workspace_hint_terms() {
        let workspace_hint = WorkspaceSearchHint {
            workspace_id: "sibyl".into(),
            matches: vec!["sibyl-memory-mvp".into()],
        };

        let terms = extract_query_artifact_terms_with_workspace(
            "servicusage.services.use in sibyl-memory-mvp",
            Some(&workspace_hint),
        );

        assert!(terms.contains(&"servicusage.services.use".to_string()));
        assert!(!terms.contains(&"sibyl-memory-mvp".to_string()));
    }

    #[test]
    fn tokenize_search_query_drops_filler_terms_and_workspace_tokens() {
        let workspace_hint = WorkspaceSearchHint {
            workspace_id: "workspace-1".into(),
            matches: vec!["sibyl-memory-mvp".into()],
        };

        let tokens = tokenize_exact_search_query_with_workspace(
            "a couple days ago we discussed lightdash in sibyl-memory-mvp",
            Some(&workspace_hint),
        );

        assert_eq!(tokens, vec!["lightdash"]);
    }

    #[test]
    fn detect_workspace_hint_matches_workspace_basename() {
        let hints = vec![
            WorkspaceSearchHint {
                workspace_id: "workspace-1".into(),
                matches: vec!["transcript-browser".into()],
            },
            WorkspaceSearchHint {
                workspace_id: "workspace-2".into(),
                matches: vec!["sibyl-memory-mvp".into()],
            },
        ];

        let hint = detect_workspace_hint(
            "a couple days ago we discussed lightdash in sibyl-memory-mvp",
            &hints,
        );

        assert_eq!(hint.map(|value| value.workspace_id.as_str()), Some("workspace-2"));
    }

    #[test]
    fn detect_recent_cutoff_ms_recognizes_relative_time_phrases() {
        let cutoff = detect_recent_cutoff_ms("a couple days ago we discussed lightdash");

        assert!(cutoff.is_some());
    }

    fn conversation_selector_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE conversation (
                id TEXT PRIMARY KEY,
                provider_conversation_id TEXT,
                parent_conversation_id TEXT,
                workspace_id TEXT,
                provider TEXT NOT NULL,
                title TEXT,
                preview_text TEXT,
                created_at_ms INTEGER,
                updated_at_ms INTEGER,
                metadata_json TEXT,
                status TEXT NOT NULL
            );
            "#,
        )
        .unwrap();
        conn
    }

    #[test]
    fn resolve_conversation_selector_matches_external_id() {
        let conn = conversation_selector_db();
        conn.execute(
            r#"
            INSERT INTO conversation (
                id, provider_conversation_id, parent_conversation_id, workspace_id, provider,
                title, preview_text, created_at_ms, updated_at_ms, metadata_json, status
            )
            VALUES (?1, ?2, NULL, NULL, 'claude_code', ?3, NULL, 0, 10, NULL, 'active')
            "#,
            rusqlite::params!["internal-1", "external-1", "Conversation One"],
        )
        .unwrap();

        let resolved = resolve_conversation_selector(&conn, "external-1").unwrap();
        assert_eq!(resolved.as_deref(), Some("internal-1"));
    }

    #[test]
    fn resolve_conversation_selector_rejects_ambiguous_exact_title() {
        let conn = conversation_selector_db();
        for (id, updated_at) in [("internal-1", 10_i64), ("internal-2", 20_i64)] {
            conn.execute(
                r#"
                INSERT INTO conversation (
                    id, provider_conversation_id, parent_conversation_id, workspace_id, provider,
                    title, preview_text, created_at_ms, updated_at_ms, metadata_json, status
                )
                VALUES (?1, NULL, NULL, NULL, 'claude_code', 'Same Title', NULL, 0, ?2, NULL, 'active')
                "#,
                rusqlite::params![id, updated_at],
            )
            .unwrap();
        }

        let error = resolve_conversation_selector(&conn, "Same Title")
            .unwrap_err()
            .to_string();
        assert!(error.contains("ambiguous"));
        assert!(error.contains("internal-1"));
        assert!(error.contains("internal-2"));
    }
}
