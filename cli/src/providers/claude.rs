use anyhow::Result;
use chrono::DateTime;
use serde_json::Value;
use shared::{
    parse_claude_scaffold_sequence, Conversation, ConversationLoadRef, Message, MessageKind,
    Participant, ProviderKind, Workspace,
};
use std::collections::{BTreeSet, HashSet};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum LoadMode {
    Full,
}

struct ParsedConversation {
    branch_parent_external_id: Option<String>,
    branch_anchor_message_id: Option<String>,
    title: Option<String>,
    preview: Option<String>,
    created_at: i64,
    updated_at: i64,
    messages: Vec<Message>,
}

#[derive(Default)]
struct ScanAccumulator {
    branch_parent_external_id: Option<String>,
    branch_anchor_message_id: Option<String>,
    messages: Vec<Message>,
    custom_title: Option<String>,
    user_preview: Option<String>,
    fallback_preview: Option<String>,
    updated_at: i64,
    created_at: i64,
}

pub fn load_workspaces_full() -> Result<Vec<Workspace>> {
    load_workspaces(LoadMode::Full)
}

fn load_workspaces(mode: LoadMode) -> Result<Vec<Workspace>> {
    let mut workspaces = Vec::new();
    let home_dir = std::env::var("HOME")?;
    let base_path = PathBuf::from(&home_dir).join(".claude/projects");
    let home_prefix = home_dir.replace("/", "-") + "-";

    if !base_path.exists() {
        return Ok(workspaces);
    }

    for entry in fs::read_dir(base_path)? {
        let entry = entry?;
        let path = entry.path();

        if !path.is_dir() {
            continue;
        }

        let dir_name = entry.file_name().to_string_lossy().to_string();
        if let Some(workspace) = load_workspace(&path, &dir_name, &home_dir, &home_prefix, mode)? {
            workspaces.push(workspace);
        }
    }

    workspaces.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    Ok(workspaces)
}

pub fn hydrate_conversation(conversation: &Conversation) -> Result<Option<Conversation>> {
    match conversation.load_ref.as_ref() {
        Some(ConversationLoadRef::ClaudeFile { path }) => {
            parse_conversation_file(Path::new(path), LoadMode::Full)
        }
        Some(ConversationLoadRef::Indexed { .. }) | None => Ok(Some(conversation.clone())),
        Some(ConversationLoadRef::CodexFiles { .. }) => Ok(Some(conversation.clone())),
    }
}

fn load_workspace(
    path: &Path,
    dir_name: &str,
    home_dir: &str,
    home_prefix: &str,
    mode: LoadMode,
) -> Result<Option<Workspace>> {
    let workspace_name = clean_project_name(path, dir_name, home_dir, home_prefix);
    let mut conversations = Vec::new();
    let mut workspace_updated_at = 0i64;

    for file_entry in fs::read_dir(path)? {
        let file_entry = file_entry?;
        let file_path = file_entry.path();

        if file_path.extension().and_then(|s| s.to_str()) != Some("jsonl") {
            continue;
        }

        if let Some(conversation) = parse_conversation_file(&file_path, mode)? {
            workspace_updated_at = workspace_updated_at.max(conversation.updated_at);
            conversations.push(conversation);
        }
    }

    if conversations.is_empty() {
        return Ok(None);
    }

    conversations.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));

    Ok(Some(Workspace {
        id: dir_name.to_string(),
        display_name: workspace_name,
        source_path: get_path_from_sessions_index(path),
        updated_at: workspace_updated_at,
        conversations,
    }))
}

fn parse_conversation_file(file_path: &Path, mode: LoadMode) -> Result<Option<Conversation>> {
    let conv_id = match file_path.file_stem().and_then(|stem| stem.to_str()) {
        Some(id) => id.to_string(),
        None => return Ok(None),
    };

    let Some(parsed) = scan_conversation_file(file_path, mode)? else {
        return Ok(None);
    };

    let short_id = conv_id.chars().take(8).collect::<String>();

    Ok(Some(Conversation {
        id: short_id,
        external_id: Some(conv_id),
        branch_parent_id: parsed.branch_parent_external_id,
        branch_anchor_message_id: parsed.branch_anchor_message_id,
        title: parsed.title,
        preview: parsed.preview,
        provider: ProviderKind::ClaudeCode,
        created_at: parsed.created_at,
        updated_at: parsed.updated_at,
        segments: vec![],
        messages: parsed.messages,
        is_hydrated: true,
        load_ref: Some(ConversationLoadRef::ClaudeFile {
            path: file_path.to_string_lossy().to_string(),
        }),
    }))
}

fn scan_conversation_file(file_path: &Path, _mode: LoadMode) -> Result<Option<ParsedConversation>> {
    let mut acc = ScanAccumulator {
        created_at: i64::MAX,
        ..Default::default()
    };

    scan_message_file(file_path, 0, true, &mut acc)?;

    let sidechain_dir = file_path.with_extension("").join("subagents");
    if sidechain_dir.exists() {
        let mut subagent_files = fs::read_dir(&sidechain_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|s| s.to_str()) == Some("jsonl"))
            .collect::<Vec<_>>();
        subagent_files.sort();

        for subagent_path in subagent_files {
            scan_message_file(&subagent_path, 1, false, &mut acc)?;
        }
    }

    acc.messages.sort_by(|left, right| {
        left.timestamp
            .unwrap_or(i64::MAX)
            .cmp(&right.timestamp.unwrap_or(i64::MAX))
    });
    dedupe_messages_by_id(&mut acc.messages);
    sanitize_message_links(&mut acc.messages);

    let preview = acc.user_preview.or(acc.fallback_preview);
    let has_content = !acc.messages.is_empty() || preview.is_some() || acc.custom_title.is_some();
    if !has_content {
        return Ok(None);
    }

    if acc.created_at == i64::MAX {
        acc.created_at = acc.updated_at;
    }

    Ok(Some(ParsedConversation {
        branch_parent_external_id: acc.branch_parent_external_id,
        branch_anchor_message_id: acc.branch_anchor_message_id,
        title: acc.custom_title,
        preview,
        created_at: acc.created_at,
        updated_at: acc.updated_at,
        messages: acc.messages,
    }))
}

fn dedupe_messages_by_id(messages: &mut Vec<Message>) {
    let mut seen_ids = HashSet::new();
    let mut deduped = Vec::with_capacity(messages.len());

    for message in messages.drain(..).rev() {
        match message.id.as_deref() {
            Some(id) if !seen_ids.insert(id.to_string()) => {}
            _ => deduped.push(message),
        }
    }

    deduped.reverse();
    *messages = deduped;
}

fn scan_message_file(
    file_path: &Path,
    depth: usize,
    allow_title: bool,
    acc: &mut ScanAccumulator,
) -> Result<()> {
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    for line in reader.lines() {
        let line = line?;
        let Ok(val) = serde_json::from_str::<Value>(&line) else {
            continue;
        };

        let timestamp_ms = val
            .get("timestamp")
            .and_then(|timestamp| timestamp.as_str())
            .and_then(parse_timestamp_millis);

        if let Some(ts_ms) = timestamp_ms {
            acc.updated_at = acc.updated_at.max(ts_ms);
            acc.created_at = acc.created_at.min(ts_ms);
        }

        if allow_title {
            if let Some(title) = val.get("customTitle").and_then(|title| title.as_str()) {
                acc.custom_title = Some(title.to_string());
            }
        }

        if acc.branch_parent_external_id.is_none() {
            let forked_from = val.get("forkedFrom");
            acc.branch_parent_external_id = forked_from
                .and_then(|forked_from| forked_from.get("sessionId"))
                .and_then(|session_id| session_id.as_str())
                .map(|session_id| session_id.to_string());
            acc.branch_anchor_message_id = forked_from
                .and_then(|forked_from| forked_from.get("messageUuid"))
                .and_then(|message_id| message_id.as_str())
                .map(|message_id| message_id.to_string());
        }

        let Some(msg) = val.get("message") else {
            continue;
        };

        let role = msg
            .get("role")
            .and_then(|role| role.as_str())
            .unwrap_or("unknown");
        let extracted = extract_message_content(msg.get("content"));
        let text_content = extracted.text;
        let thinking_blocks = extracted.thinking;
        if text_content.is_none() && thinking_blocks.is_empty() {
            continue;
        }

        let is_sidechain = val
            .get("isSidechain")
            .and_then(|value| value.as_bool())
            .unwrap_or(false);

        let message_id = val
            .get("uuid")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let parent_id = val
            .get("parentUuid")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());
        let associated_id = val
            .get("sourceToolAssistantUUID")
            .and_then(|value| value.as_str())
            .map(|value| value.to_string());

        let (participant, kind) = classify_claude_message(role, is_sidechain || depth > 0);

        for (idx, thinking_content) in thinking_blocks.into_iter().enumerate() {
            acc.messages.push(Message {
                id: message_id.as_ref().map(|id| format!("{id}:thinking:{idx}")),
                kind: MessageKind::Thinking,
                participant: Participant::Assistant {
                    provider: ProviderKind::ClaudeCode,
                },
                content: thinking_content,
                timestamp: timestamp_ms,
                parent_id: parent_id.clone(),
                associated_id: message_id.clone().or_else(|| associated_id.clone()),
                depth,
            });
        }

        if let Some(text_content) = text_content {
            if acc.fallback_preview.is_none()
                && parse_claude_scaffold_sequence(&text_content).is_none()
            {
                acc.fallback_preview = first_non_empty_line(&text_content).map(str::to_string);
            }
            if acc.user_preview.is_none()
                && role.eq_ignore_ascii_case("user")
                && !(is_sidechain || depth > 0)
                && parse_claude_scaffold_sequence(&text_content).is_none()
            {
                acc.user_preview = first_non_empty_line(&text_content).map(str::to_string);
            }

            acc.messages.push(Message {
                id: message_id,
                kind,
                participant,
                content: text_content,
                timestamp: timestamp_ms,
                parent_id,
                associated_id,
                depth,
            });
        }
    }

    Ok(())
}

fn classify_claude_message(role: &str, is_sidechain: bool) -> (Participant, MessageKind) {
    if is_sidechain && role.eq_ignore_ascii_case("user") {
        return (Participant::System, MessageKind::MetadataChange);
    }

    let participant = Participant::from_role(role, ProviderKind::ClaudeCode);
    let kind = match participant {
        Participant::User => MessageKind::UserMessage,
        Participant::Assistant { .. } => MessageKind::AssistantMessage,
        Participant::Tool { .. } => MessageKind::ToolResult,
        Participant::System | Participant::Unknown { .. } => MessageKind::MetadataChange,
    };
    (participant, kind)
}

fn sanitize_message_links(messages: &mut [Message]) {
    let ids = messages
        .iter()
        .filter_map(|message| message.id.clone())
        .collect::<BTreeSet<_>>();

    for message in messages {
        if message
            .parent_id
            .as_ref()
            .is_some_and(|parent_id| !ids.contains(parent_id))
        {
            message.parent_id = None;
        }
        if message
            .associated_id
            .as_ref()
            .is_some_and(|associated_id| !ids.contains(associated_id))
        {
            message.associated_id = None;
        }
    }
}

fn get_path_from_sessions_index(project_dir: &Path) -> Option<String> {
    let index_path = project_dir.join("sessions-index.json");
    if let Ok(content) = fs::read_to_string(index_path) {
        if let Ok(json) = serde_json::from_str::<Value>(&content) {
            if let Some(entries) = json.get("entries").and_then(|entries| entries.as_array()) {
                if let Some(first) = entries.first() {
                    if let Some(project_path) = first
                        .get("projectPath")
                        .and_then(|project_path| project_path.as_str())
                    {
                        return Some(project_path.to_string());
                    }
                }
            }
        }
    }
    None
}

fn reconstruct_path_recursive(base_dir: &Path, parts: &[&str]) -> Option<PathBuf> {
    if parts.is_empty() {
        return Some(base_dir.to_path_buf());
    }

    for i in 1..=parts.len() {
        let candidate_part = parts[..i].join("-");
        let mut candidates = vec![candidate_part.clone()];

        if candidate_part.starts_with('-') && candidate_part.len() > 1 {
            candidates.push(format!(".{}", &candidate_part[1..]));
        }
        if candidate_part.contains("--") {
            candidates.push(candidate_part.replace("--", "-."));
        }

        for candidate in candidates {
            let candidate_path = base_dir.join(&candidate);
            if candidate_path.exists() {
                if let Some(result) = reconstruct_path_recursive(&candidate_path, &parts[i..]) {
                    return Some(result);
                }
            }
        }
    }

    None
}

pub(crate) fn clean_project_name(
    project_dir: &Path,
    dir_name: &str,
    home_dir: &str,
    home_prefix: &str,
) -> String {
    if let Some(real_path) = get_path_from_sessions_index(project_dir) {
        if real_path.starts_with(home_dir) {
            return format!("~{}", &real_path[home_dir.len()..]);
        }
        return real_path;
    }

    if dir_name.starts_with(home_prefix) {
        let rest = &dir_name[home_prefix.len()..];
        let parts: Vec<&str> = rest.split('-').collect();

        if let Some(real_path) =
            reconstruct_path_recursive(PathBuf::from(home_dir).as_path(), &parts)
        {
            let real_str = real_path.to_string_lossy();
            if real_str.starts_with(home_dir) {
                return format!("~{}", &real_str[home_dir.len()..]);
            }
            return real_str.to_string();
        }
    }

    let mut project_name = dir_name.replace("--", "-.");
    project_name = project_name.trim_start_matches(home_prefix).to_string();
    let formatted_path = project_name.replace("-", "/");
    format!("~/{}", formatted_path)
}

struct ExtractedClaudeContent {
    text: Option<String>,
    thinking: Vec<String>,
}

fn extract_message_content(content: Option<&Value>) -> ExtractedClaudeContent {
    let Some(content) = content else {
        return ExtractedClaudeContent {
            text: None,
            thinking: Vec::new(),
        };
    };

    let mut text_content = String::new();
    let mut thinking_content = Vec::new();
    if let Some(text) = content.as_str() {
        text_content = text.to_string();
    } else if let Some(arr) = content.as_array() {
        for item in arr {
            if let Some(obj) = item.as_object() {
                match obj.get("type").and_then(|value| value.as_str()) {
                    Some("text") => {
                        if let Some(text) = obj.get("text").and_then(|value| value.as_str()) {
                            if !text_content.is_empty() {
                                text_content.push_str("\n\n");
                            }
                            text_content.push_str(text);
                        }
                    }
                    Some("thinking") => {
                        if let Some(text) = obj
                            .get("thinking")
                            .and_then(|value| value.as_str())
                            .or_else(|| obj.get("text").and_then(|value| value.as_str()))
                        {
                            let trimmed = text.trim();
                            if !trimmed.is_empty() {
                                thinking_content.push(trimmed.to_string());
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    ExtractedClaudeContent {
        text: (!text_content.is_empty()).then_some(text_content),
        thinking: thinking_content,
    }
}

fn parse_timestamp_millis(timestamp: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn first_non_empty_line(content: &str) -> Option<&str> {
    content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && parse_claude_scaffold_sequence(line).is_none())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_temp_dir(name: &str) -> PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!("transcript-browser-{name}-{suffix}"))
    }

    #[test]
    fn test_clean_project_name() {
        let home_dir = "/Users/gaborkerekes";
        let home_prefix = "-Users-gaborkerekes-";
        let dummy_path = PathBuf::from("/tmp");

        assert_eq!(
            clean_project_name(
                &dummy_path,
                "-Users-gaborkerekes--config-konfigue",
                home_dir,
                home_prefix,
            ),
            "~/.config/konfigue"
        );

        assert_eq!(
            clean_project_name(
                &dummy_path,
                "-Users-gaborkerekes-git-jj-stuff-jjui",
                home_dir,
                home_prefix,
            ),
            "~/git/jj-stuff/jjui"
        );
    }

    #[test]
    fn test_extract_message_content_from_string() {
        let content = Value::String("hello".into());
        let extracted = extract_message_content(Some(&content));
        assert_eq!(extracted.text.as_deref(), Some("hello"));
        assert!(extracted.thinking.is_empty());
    }

    #[test]
    fn test_extract_message_content_from_text_blocks() {
        let content = serde_json::json!([
            {"type": "text", "text": "hello"},
            {"type": "tool_use", "name": "Read"},
            {"type": "text", "text": "world"}
        ]);
        let extracted = extract_message_content(Some(&content));
        assert_eq!(extracted.text.as_deref(), Some("hello\n\nworld"));
        assert!(extracted.thinking.is_empty());
    }

    #[test]
    fn test_extract_message_content_extracts_thinking_blocks() {
        let content = serde_json::json!([
            {"type": "tool_use", "name": "Read"},
            {"type": "thinking", "text": "hidden"}
        ]);
        let extracted = extract_message_content(Some(&content));
        assert_eq!(extracted.text, None);
        assert_eq!(extracted.thinking, vec!["hidden".to_string()]);
    }

    #[test]
    fn parse_conversation_file_preserves_thinking_entries() {
        let temp_dir = unique_temp_dir("claude-thinking");
        fs::create_dir_all(&temp_dir).unwrap();

        let path = temp_dir.join("conv-thinking.jsonl");
        fs::write(
            &path,
            concat!(
                "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"m1\",\"message\":{\"role\":\"assistant\",\"content\":[",
                "{\"type\":\"thinking\",\"text\":\"first thought\"},",
                "{\"type\":\"text\",\"text\":\"final answer\"}]}}\n"
            ),
        )
        .unwrap();

        let conversation = parse_conversation_file(&path, LoadMode::Full)
            .unwrap()
            .expect("conversation");

        assert_eq!(conversation.messages.len(), 2);
        assert_eq!(conversation.messages[0].kind, MessageKind::Thinking);
        assert_eq!(conversation.messages[0].content, "first thought");
        assert_eq!(conversation.messages[1].kind, MessageKind::AssistantMessage);
        assert_eq!(conversation.messages[1].content, "final answer");
    }

    #[test]
    fn parse_conversation_file_merges_subagent_sidechains() {
        let temp_dir = unique_temp_dir("claude-subagents");
        fs::create_dir_all(&temp_dir).unwrap();

        let main_path = temp_dir.join("conv-1.jsonl");
        let subagent_dir = temp_dir.join("conv-1").join("subagents");
        fs::create_dir_all(&subagent_dir).unwrap();
        let subagent_path = subagent_dir.join("agent-a.jsonl");

        fs::write(
            &main_path,
            concat!(
                "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"root-1\",\"message\":{\"role\":\"user\",\"content\":\"hello main\"}}\n",
                "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"root-2\",\"parentUuid\":\"root-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"main reply\"}]}}\n"
            ),
        )
        .unwrap();

        fs::write(
            &subagent_path,
            concat!(
                "{\"timestamp\":\"2026-01-01T00:00:02Z\",\"uuid\":\"child-1\",\"parentUuid\":\"root-2\",\"sourceToolAssistantUUID\":\"root-2\",\"isSidechain\":true,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"subagent reply\"}]}}\n"
            ),
        )
        .unwrap();

        let conversation = parse_conversation_file(&main_path, LoadMode::Full)
            .unwrap()
            .unwrap();

        assert_eq!(conversation.messages.len(), 3);
        assert_eq!(conversation.messages[2].content, "subagent reply");
        assert_eq!(conversation.messages[2].id.as_deref(), Some("child-1"));
        assert_eq!(
            conversation.messages[2].parent_id.as_deref(),
            Some("root-2")
        );
        assert_eq!(
            conversation.messages[2].associated_id.as_deref(),
            Some("root-2")
        );
        assert_eq!(conversation.messages[2].depth, 1);

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn parse_conversation_file_reclassifies_sidechain_user_prompt_as_metadata() {
        let temp_dir = unique_temp_dir("claude-sidechain-user");
        fs::create_dir_all(&temp_dir).unwrap();

        let main_path = temp_dir.join("conv-1.jsonl");
        let subagent_dir = temp_dir.join("conv-1").join("subagents");
        fs::create_dir_all(&subagent_dir).unwrap();
        let subagent_path = subagent_dir.join("agent-a.jsonl");

        fs::write(
            &main_path,
            concat!(
                "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"root-1\",\"message\":{\"role\":\"user\",\"content\":\"hello main\"}}\n",
                "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"root-2\",\"parentUuid\":\"root-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"main reply\"}]}}\n"
            ),
        )
        .unwrap();

        fs::write(
            &subagent_path,
            concat!(
                "{\"timestamp\":\"2026-01-01T00:00:02Z\",\"uuid\":\"child-0\",\"isSidechain\":true,\"message\":{\"role\":\"user\",\"content\":\"delegated prompt\"}}\n",
                "{\"timestamp\":\"2026-01-01T00:00:03Z\",\"uuid\":\"child-1\",\"parentUuid\":\"child-0\",\"sourceToolAssistantUUID\":\"root-2\",\"isSidechain\":true,\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"subagent reply\"}]}}\n"
            ),
        )
        .unwrap();

        let conversation = parse_conversation_file(&main_path, LoadMode::Full)
            .unwrap()
            .unwrap();

        let delegated = conversation
            .messages
            .iter()
            .find(|message| message.id.as_deref() == Some("child-0"))
            .unwrap();

        assert_eq!(delegated.participant, Participant::System);
        assert_eq!(delegated.kind, MessageKind::MetadataChange);
        assert_eq!(delegated.content, "delegated prompt");

        fs::remove_dir_all(temp_dir).unwrap();
    }

    #[test]
    fn parse_conversation_file_dedupes_replayed_uuids_by_keeping_last_occurrence() {
        let temp_dir = unique_temp_dir("claude-dedupe");
        fs::create_dir_all(&temp_dir).unwrap();

        let main_path = temp_dir.join("conv-1.jsonl");

        fs::write(
            &main_path,
            concat!(
                "{\"timestamp\":\"2026-01-01T00:00:00Z\",\"uuid\":\"root-1\",\"message\":{\"role\":\"user\",\"content\":\"root\"}}\n",
                "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"dup-1\",\"parentUuid\":\"old-parent\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"same reply\"}]}}\n",
                "{\"timestamp\":\"2026-01-01T00:00:01Z\",\"uuid\":\"dup-1\",\"parentUuid\":\"root-1\",\"message\":{\"role\":\"assistant\",\"content\":[{\"type\":\"text\",\"text\":\"same reply\"}]}}\n"
            ),
        )
        .unwrap();

        let conversation = parse_conversation_file(&main_path, LoadMode::Full)
            .unwrap()
            .unwrap();

        assert_eq!(conversation.messages.len(), 2);
        assert_eq!(conversation.messages[1].id.as_deref(), Some("dup-1"));
        assert_eq!(
            conversation.messages[1].parent_id.as_deref(),
            Some("root-1")
        );

        fs::remove_dir_all(temp_dir).unwrap();
    }
}
