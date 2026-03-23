use anyhow::Result;
use chrono::DateTime;
use serde::Deserialize;
use serde_json::Value;
use shared::{
    Conversation, ConversationLoadRef, ConversationSegment, Message, MessageKind, Participant,
    ProviderKind, Workspace,
};
use std::collections::BTreeMap;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum LoadMode {
    Full,
}

#[derive(Debug)]
struct ParsedSessionFile {
    workspace_path: Option<String>,
    session_id: String,
    title: Option<String>,
    segment_id: String,
    segment_label: String,
    created_at: i64,
    updated_at: i64,
    preview: Option<String>,
    message_count: usize,
    file_path: String,
    messages: Vec<Message>,
}

#[derive(Debug, Deserialize)]
struct CodexLine {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    entry_type: String,
    payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct SessionMeta {
    id: String,
    cwd: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ResponseItemPayload {
    #[serde(rename = "type")]
    payload_type: String,
    role: Option<String>,
    content: Option<Vec<ContentBlock>>,
}

#[derive(Debug, Deserialize)]
struct ContentBlock {
    #[serde(rename = "type")]
    content_type: String,
    text: Option<String>,
}

pub fn load_workspaces_full() -> Result<Vec<Workspace>> {
    load_workspaces(LoadMode::Full)
}

fn load_workspaces(mode: LoadMode) -> Result<Vec<Workspace>> {
    let home_dir = std::env::var("HOME")?;
    let base_path = PathBuf::from(&home_dir).join(".codex/sessions");

    if !base_path.exists() {
        return Ok(Vec::new());
    }

    let mut parsed_sessions = Vec::new();
    for file_path in discover_session_files(&base_path)? {
        if let Some(parsed) = parse_session_file(&file_path, mode)? {
            parsed_sessions.push(parsed);
        }
    }

    Ok(build_workspaces_from_sessions(parsed_sessions, &home_dir))
}

pub fn hydrate_conversation(conversation: &Conversation) -> Result<Option<Conversation>> {
    match conversation.load_ref.as_ref() {
        Some(ConversationLoadRef::CodexFiles { paths }) => {
            let home_dir = std::env::var("HOME")?;
            let mut parsed_sessions = Vec::new();
            for path in paths {
                if let Some(parsed) = parse_session_file(Path::new(path), LoadMode::Full)? {
                    parsed_sessions.push(parsed);
                }
            }

            let workspaces = build_workspaces_from_sessions(parsed_sessions, &home_dir);
            Ok(workspaces
                .into_iter()
                .flat_map(|workspace| workspace.conversations.into_iter())
                .next())
        }
        Some(ConversationLoadRef::Indexed { .. }) | None => Ok(Some(conversation.clone())),
        Some(ConversationLoadRef::ClaudeFile { .. }) => Ok(Some(conversation.clone())),
    }
}

fn build_workspaces_from_sessions(
    parsed_sessions: Vec<ParsedSessionFile>,
    home_dir: &str,
) -> Vec<Workspace> {
    let mut by_path: BTreeMap<String, WorkspaceSessions> = BTreeMap::new();

    for parsed in parsed_sessions {
        let ParsedSessionFile {
            workspace_path,
            session_id,
            title,
            segment_id,
            segment_label,
            created_at,
            updated_at,
            preview,
            message_count,
            file_path,
            messages,
        } = parsed;
        let workspace_key = workspace_path.unwrap_or_else(|| ".".to_string());
        let display_name = prettify_path(&workspace_key, home_dir);
        let workspace = by_path
            .entry(workspace_key.clone())
            .or_insert_with(|| WorkspaceSessions {
                display_name,
                source_path: if workspace_key == "." {
                    None
                } else {
                    Some(workspace_key.clone())
                },
                updated_at: 0,
                sessions: BTreeMap::new(),
            });
        workspace.updated_at = workspace.updated_at.max(updated_at);
        let session = workspace
            .sessions
            .entry(session_id.clone())
            .or_insert_with(|| SessionAccumulator::new(session_id));
        session.merge(PendingSegment {
            id: segment_id,
            label: segment_label,
            created_at,
            updated_at,
            preview,
            message_count,
            file_path,
            messages,
            order: session.next_segment_order,
        });
        if session.title.is_none() {
            session.title = title;
        }
    }

    let mut workspaces: Vec<Workspace> = by_path
        .into_iter()
        .map(|(workspace_key, sessions)| sessions.into_workspace(workspace_key))
        .collect();
    for workspace in &mut workspaces {
        workspace
            .conversations
            .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    }
    workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    workspaces
}

struct WorkspaceSessions {
    display_name: String,
    source_path: Option<String>,
    updated_at: i64,
    sessions: BTreeMap<String, SessionAccumulator>,
}

impl WorkspaceSessions {
    fn into_workspace(self, workspace_key: String) -> Workspace {
        let conversations = self
            .sessions
            .into_values()
            .map(SessionAccumulator::into_conversation)
            .collect();

        Workspace {
            id: workspace_key,
            display_name: self.display_name,
            source_path: self.source_path,
            updated_at: self.updated_at,
            conversations,
        }
    }
}

struct SessionAccumulator {
    session_id: String,
    title: Option<String>,
    created_at: i64,
    updated_at: i64,
    segments: Vec<PendingSegment>,
    next_segment_order: usize,
}

struct PendingSegment {
    id: String,
    label: String,
    created_at: i64,
    updated_at: i64,
    preview: Option<String>,
    message_count: usize,
    file_path: String,
    messages: Vec<Message>,
    order: usize,
}

impl SessionAccumulator {
    fn new(session_id: String) -> Self {
        Self {
            session_id,
            title: None,
            created_at: i64::MAX,
            updated_at: 0,
            segments: Vec::new(),
            next_segment_order: 0,
        }
    }

    fn merge(&mut self, mut segment: PendingSegment) {
        self.created_at = self.created_at.min(segment.created_at);
        self.updated_at = self.updated_at.max(segment.updated_at);
        segment.order = self.next_segment_order;
        self.next_segment_order += 1;
        self.segments.push(segment);
    }

    fn into_conversation(mut self) -> Conversation {
        self.segments.sort_by(|left, right| {
            (left.created_at, left.order).cmp(&(right.created_at, right.order))
        });

        let mut messages = Vec::new();
        let mut segments = Vec::new();
        let mut preview = None;
        let mut file_paths = Vec::new();
        let mut total_message_count = 0usize;

        for mut segment in self.segments {
            segment.messages.sort_by(|left, right| {
                left.timestamp
                    .unwrap_or(i64::MAX)
                    .cmp(&right.timestamp.unwrap_or(i64::MAX))
            });

            if preview.is_none() {
                preview = segment.preview.clone();
            }

            let message_count = if segment.messages.is_empty() {
                segment.message_count
            } else {
                segment.messages.len()
            };
            let message_start_idx = total_message_count;
            total_message_count += message_count;
            file_paths.push(segment.file_path);
            messages.extend(segment.messages);

            segments.push(ConversationSegment {
                id: segment.id,
                label: segment.label,
                created_at: segment.created_at,
                updated_at: segment.updated_at,
                message_start_idx,
                message_count,
            });
        }

        let created_at = if self.created_at == i64::MAX {
            self.updated_at
        } else {
            self.created_at
        };

        let is_hydrated = total_message_count == 0 || total_message_count == messages.len();

        Conversation {
            id: self.session_id.chars().take(8).collect(),
            external_id: Some(self.session_id),
            title: self.title,
            preview,
            provider: ProviderKind::Codex,
            created_at,
            updated_at: self.updated_at,
            segments,
            messages,
            is_hydrated,
            load_ref: Some(ConversationLoadRef::CodexFiles { paths: file_paths }),
        }
    }
}

fn discover_session_files(base_path: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    visit_dirs(base_path, &mut files)?;
    files.sort();
    Ok(files)
}

fn visit_dirs(dir: &Path, files: &mut Vec<PathBuf>) -> Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            visit_dirs(&path, files)?;
            continue;
        }
        if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            files.push(path);
        }
    }
    Ok(())
}

fn parse_session_file(file_path: &Path, _mode: LoadMode) -> Result<Option<ParsedSessionFile>> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);
    let file_stem = match file_path.file_stem().and_then(|stem| stem.to_str()) {
        Some(stem) => stem.to_string(),
        None => return Ok(None),
    };

    let mut external_id = None;
    let mut cwd = None;
    let mut messages = Vec::new();
    let mut user_preview = None;
    let mut fallback_preview = None;
    let mut message_count = 0usize;
    let mut created_at = i64::MAX;
    let mut updated_at = 0i64;

    for line in reader.lines() {
        let line = line?;
        let Ok(entry) = serde_json::from_str::<CodexLine>(&line) else {
            continue;
        };

        let timestamp_ms = entry.timestamp.as_deref().and_then(parse_timestamp_millis);

        match entry.entry_type.as_str() {
            "session_meta" => {
                let Some(payload) = entry.payload else {
                    continue;
                };
                let Ok(meta) = serde_json::from_value::<SessionMeta>(payload) else {
                    continue;
                };
                external_id = Some(meta.id);
                cwd = meta.cwd;
            }
            "response_item" => {
                let Some(payload) = entry.payload else {
                    continue;
                };
                let Ok(item) = serde_json::from_value::<ResponseItemPayload>(payload) else {
                    continue;
                };
                if item.payload_type != "message" {
                    continue;
                }
                let Some((participant, content)) = parse_response_item_content(item) else {
                    continue;
                };
                message_count += 1;
                if fallback_preview.is_none() {
                    fallback_preview = first_non_empty_line(&content).map(str::to_string);
                }
                if user_preview.is_none() && participant.is_user() {
                    user_preview = first_non_empty_line(&content).map(str::to_string);
                }
                if let Some(ts) = timestamp_ms {
                    created_at = created_at.min(ts);
                    updated_at = updated_at.max(ts);
                }
                messages.push(Message {
                    id: None,
                    kind: match participant {
                        Participant::User => MessageKind::UserMessage,
                        Participant::Assistant { .. } => MessageKind::AssistantMessage,
                        Participant::Tool { .. } => MessageKind::ToolResult,
                        Participant::System | Participant::Unknown { .. } => {
                            MessageKind::MetadataChange
                        }
                    },
                    participant,
                    content,
                    timestamp: timestamp_ms,
                    parent_id: None,
                    associated_id: None,
                    depth: 0,
                });
            }
            _ => {}
        }
    }

    if message_count == 0 {
        return Ok(None);
    }

    if created_at == i64::MAX {
        created_at = updated_at;
    }

    let external_id = external_id.unwrap_or_else(|| file_stem.clone());
    let title = load_title_from_session_index(&external_id)?;

    Ok(Some(ParsedSessionFile {
        workspace_path: cwd,
        session_id: external_id.clone(),
        title,
        segment_id: file_stem.clone(),
        segment_label: build_segment_label(&file_stem, &external_id),
        created_at,
        updated_at,
        preview: user_preview.or(fallback_preview),
        message_count,
        file_path: file_path.to_string_lossy().to_string(),
        messages,
    }))
}

fn build_segment_label(file_stem: &str, session_id: &str) -> String {
    if file_stem.contains(session_id) {
        return "main session".to_string();
    }

    let suffix = file_stem.rsplit('-').next().unwrap_or(file_stem);
    if suffix.len() >= 8 {
        format!("rollout {}...", &suffix[..8])
    } else {
        file_stem.to_string()
    }
}

fn parse_response_item_content(item: ResponseItemPayload) -> Option<(Participant, String)> {
    let participant = match item.role.as_deref() {
        Some("user") => Participant::User,
        Some("assistant") => Participant::Assistant {
            provider: ProviderKind::Codex,
        },
        Some("system") | Some("developer") => Participant::System,
        Some("tool") => Participant::Tool { name: None },
        Some(other) => Participant::Unknown {
            raw_role: other.to_string(),
        },
        None => infer_participant_from_content(&item.content)?,
    };

    let content = extract_content(item.content?)?;
    Some((participant, content))
}

fn infer_participant_from_content(content: &Option<Vec<ContentBlock>>) -> Option<Participant> {
    let content = content.as_ref()?;
    if content
        .iter()
        .any(|block| block.content_type == "input_text")
    {
        Some(Participant::User)
    } else if content
        .iter()
        .any(|block| block.content_type == "output_text")
    {
        Some(Participant::Assistant {
            provider: ProviderKind::Codex,
        })
    } else {
        None
    }
}

fn extract_content(content: Vec<ContentBlock>) -> Option<String> {
    let texts: Vec<String> = content
        .into_iter()
        .filter_map(|block| match block.content_type.as_str() {
            "input_text" | "output_text" => block.text,
            _ => None,
        })
        .filter(|text| !is_injected_block(text))
        .collect();

    let joined = texts.join("\n");
    let trimmed = joined.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn is_injected_block(text: &str) -> bool {
    let trimmed = text.trim();
    (trimmed.starts_with("# AGENTS.md instructions for ") && trimmed.ends_with("</INSTRUCTIONS>"))
        || (trimmed.starts_with("<environment_context>")
            && trimmed.ends_with("</environment_context>"))
        || (trimmed.starts_with("<user_instructions>") && trimmed.ends_with("</user_instructions>"))
        || (trimmed.starts_with("<permissions instructions>")
            && trimmed.ends_with("</permissions instructions>"))
        || (trimmed.starts_with("<skills_instructions>")
            && trimmed.ends_with("</skills_instructions>"))
}

fn load_title_from_session_index(session_id: &str) -> Result<Option<String>> {
    let home_dir = std::env::var("HOME")?;
    let index_path = PathBuf::from(home_dir).join(".codex/session_index.jsonl");
    if !index_path.exists() {
        return Ok(None);
    }
    let content = fs::read_to_string(index_path)?;
    for line in content.lines() {
        let Ok(value) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        let id_matches = value.get("id").and_then(|id| id.as_str()) == Some(session_id);
        if !id_matches {
            continue;
        }
        let title = value
            .get("thread_name")
            .and_then(|name| name.as_str())
            .map(|name| name.to_string());
        return Ok(title);
    }
    Ok(None)
}

fn parse_timestamp_millis(timestamp: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn prettify_path(path: &str, home_dir: &str) -> String {
    if path == "." {
        return "Unknown Workspace".to_string();
    }
    if let Some(suffix) = path.strip_prefix(home_dir) {
        format!("~{}", suffix)
    } else {
        path.to_string()
    }
}

fn first_non_empty_line(content: &str) -> Option<&str> {
    content.lines().map(str::trim).find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(content: &str, timestamp: i64) -> Message {
        Message {
            id: None,
            kind: MessageKind::UserMessage,
            participant: Participant::User,
            content: content.into(),
            timestamp: Some(timestamp),
            parent_id: None,
            associated_id: None,
            depth: 0,
        }
    }

    #[test]
    fn test_extract_content_filters_injected_blocks() {
        let content = vec![
            ContentBlock {
                content_type: "input_text".into(),
                text: Some(
                    "# AGENTS.md instructions for /tmp\n\n<INSTRUCTIONS>\nfoo\n</INSTRUCTIONS>"
                        .into(),
                ),
            },
            ContentBlock {
                content_type: "input_text".into(),
                text: Some("actual user message".into()),
            },
        ];
        assert_eq!(extract_content(content), Some("actual user message".into()));
    }

    #[test]
    fn test_extract_content_joins_supported_blocks() {
        let content = vec![
            ContentBlock {
                content_type: "input_text".into(),
                text: Some("hello".into()),
            },
            ContentBlock {
                content_type: "tool_result".into(),
                text: Some("ignore".into()),
            },
            ContentBlock {
                content_type: "output_text".into(),
                text: Some("world".into()),
            },
        ];
        assert_eq!(extract_content(content), Some("hello\nworld".into()));
    }

    #[test]
    fn test_parse_response_item_maps_developer_to_system() {
        let item = ResponseItemPayload {
            payload_type: "message".into(),
            role: Some("developer".into()),
            content: Some(vec![ContentBlock {
                content_type: "input_text".into(),
                text: Some("developer content".into()),
            }]),
        };
        let (participant, content) = parse_response_item_content(item).expect("message");
        assert_eq!(participant, Participant::System);
        assert_eq!(content, "developer content");
    }

    #[test]
    fn test_build_workspaces_groups_rollout_files_by_session_id() {
        let workspaces = build_workspaces_from_sessions(
            vec![
                ParsedSessionFile {
                    workspace_path: Some("/Users/test/project".into()),
                    session_id: "session-12345678".into(),
                    title: Some("dump-screen".into()),
                    segment_id: "rollout-2026-main-session-12345678".into(),
                    segment_label: "main session".into(),
                    created_at: 100,
                    updated_at: 200,
                    preview: Some("first file".into()),
                    message_count: 1,
                    file_path: "/tmp/main.jsonl".into(),
                    messages: vec![message("first file", 200)],
                },
                ParsedSessionFile {
                    workspace_path: Some("/Users/test/project".into()),
                    session_id: "session-12345678".into(),
                    title: None,
                    segment_id: "rollout-2026-abcdef12".into(),
                    segment_label: "rollout abcdef12...".into(),
                    created_at: 50,
                    updated_at: 300,
                    preview: Some("second file early".into()),
                    message_count: 2,
                    file_path: "/tmp/rollout.jsonl".into(),
                    messages: vec![
                        message("second file early", 50),
                        message("second file late", 300),
                    ],
                },
            ],
            "/Users/test",
        );

        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].conversations.len(), 1);

        let conversation = &workspaces[0].conversations[0];
        assert_eq!(
            conversation.external_id.as_deref(),
            Some("session-12345678")
        );
        assert_eq!(conversation.title.as_deref(), Some("dump-screen"));
        assert_eq!(conversation.preview.as_deref(), Some("second file early"));
        assert_eq!(conversation.created_at, 50);
        assert_eq!(conversation.updated_at, 300);
        assert_eq!(conversation.segments.len(), 2);
        assert_eq!(conversation.segments[0].label, "rollout abcdef12...");
        assert_eq!(conversation.segments[0].message_start_idx, 0);
        assert_eq!(conversation.segments[0].message_count, 2);
        assert_eq!(conversation.segments[1].label, "main session");
        assert_eq!(conversation.segments[1].message_start_idx, 2);
        assert_eq!(conversation.segments[1].message_count, 1);
        assert_eq!(
            conversation
                .messages
                .iter()
                .map(|message| message.content.as_str())
                .collect::<Vec<_>>(),
            vec!["second file early", "second file late", "first file"]
        );
    }

    #[test]
    fn test_build_workspaces_does_not_merge_same_session_id_across_workspaces() {
        let workspaces = build_workspaces_from_sessions(
            vec![
                ParsedSessionFile {
                    workspace_path: Some("/Users/test/project-a".into()),
                    session_id: "session-12345678".into(),
                    title: None,
                    segment_id: "file-a".into(),
                    segment_label: "main session".into(),
                    created_at: 100,
                    updated_at: 200,
                    preview: Some("a".into()),
                    message_count: 1,
                    file_path: "/tmp/a.jsonl".into(),
                    messages: vec![message("a", 100)],
                },
                ParsedSessionFile {
                    workspace_path: Some("/Users/test/project-b".into()),
                    session_id: "session-12345678".into(),
                    title: None,
                    segment_id: "file-b".into(),
                    segment_label: "main session".into(),
                    created_at: 150,
                    updated_at: 250,
                    preview: Some("b".into()),
                    message_count: 1,
                    file_path: "/tmp/b.jsonl".into(),
                    messages: vec![message("b", 150)],
                },
            ],
            "/Users/test",
        );

        assert_eq!(workspaces.len(), 2);
        assert!(workspaces
            .iter()
            .all(|workspace| workspace.conversations.len() == 1));
    }
}
