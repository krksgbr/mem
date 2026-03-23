use anyhow::Result;
use chrono::DateTime;
use serde_json::Value;
use shared::{
    Conversation, ConversationLoadRef, Message, MessageKind, Participant, ProviderKind, Workspace,
};
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy)]
enum LoadMode {
    Full,
}

struct ParsedConversation {
    title: Option<String>,
    preview: Option<String>,
    created_at: i64,
    updated_at: i64,
    messages: Vec<Message>,
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
    let file = File::open(file_path)?;
    let reader = BufReader::new(file);

    let mut messages = Vec::new();
    let mut custom_title = None;
    let mut user_preview = None;
    let mut fallback_preview = None;
    let mut conv_updated_at = 0i64;
    let mut conv_created_at = i64::MAX;

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
            conv_updated_at = conv_updated_at.max(ts_ms);
            conv_created_at = conv_created_at.min(ts_ms);
        }

        if let Some(title) = val.get("customTitle").and_then(|title| title.as_str()) {
            custom_title = Some(title.to_string());
        }

        let Some(msg) = val.get("message") else {
            continue;
        };

        let role = msg
            .get("role")
            .and_then(|role| role.as_str())
            .unwrap_or("unknown");
        let Some(text_content) = extract_message_content(msg.get("content")) else {
            continue;
        };

        if fallback_preview.is_none() {
            fallback_preview = first_non_empty_line(&text_content).map(str::to_string);
        }
        if user_preview.is_none() && role.eq_ignore_ascii_case("user") {
            user_preview = first_non_empty_line(&text_content).map(str::to_string);
        }

        let participant = Participant::from_role(role, ProviderKind::ClaudeCode);
        messages.push(Message {
            id: None,
            kind: match participant {
                Participant::User => MessageKind::UserMessage,
                Participant::Assistant { .. } => MessageKind::AssistantMessage,
                Participant::Tool { .. } => MessageKind::ToolResult,
                Participant::System | Participant::Unknown { .. } => MessageKind::MetadataChange,
            },
            participant,
            content: text_content,
            timestamp: timestamp_ms,
            parent_id: None,
            associated_id: None,
            depth: 0,
        });
    }

    let preview = user_preview.or(fallback_preview);
    let has_content = !messages.is_empty() || preview.is_some() || custom_title.is_some();
    if !has_content {
        return Ok(None);
    }

    if conv_created_at == i64::MAX {
        conv_created_at = conv_updated_at;
    }

    Ok(Some(ParsedConversation {
        title: custom_title,
        preview,
        created_at: conv_created_at,
        updated_at: conv_updated_at,
        messages,
    }))
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

fn extract_message_content(content: Option<&Value>) -> Option<String> {
    let Some(content) = content else {
        return None;
    };

    let mut text_content = String::new();
    if let Some(text) = content.as_str() {
        text_content = text.to_string();
    } else if let Some(arr) = content.as_array() {
        for item in arr {
            if let Some(obj) = item.as_object() {
                if obj.get("type").and_then(|value| value.as_str()) != Some("text") {
                    continue;
                }
                if let Some(text) = obj.get("text").and_then(|value| value.as_str()) {
                    if !text_content.is_empty() {
                        text_content.push_str("\n\n");
                    }
                    text_content.push_str(text);
                }
            }
        }
    }

    if text_content.is_empty() {
        None
    } else {
        Some(text_content)
    }
}

fn parse_timestamp_millis(timestamp: &str) -> Option<i64> {
    DateTime::parse_from_rfc3339(timestamp)
        .ok()
        .map(|dt| dt.timestamp_millis())
}

fn first_non_empty_line(content: &str) -> Option<&str> {
    content.lines().map(str::trim).find(|line| !line.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(
            extract_message_content(Some(&content)),
            Some("hello".into())
        );
    }

    #[test]
    fn test_extract_message_content_from_text_blocks() {
        let content = serde_json::json!([
            {"type": "text", "text": "hello"},
            {"type": "tool_use", "name": "Read"},
            {"type": "text", "text": "world"}
        ]);
        assert_eq!(
            extract_message_content(Some(&content)),
            Some("hello\n\nworld".into())
        );
    }

    #[test]
    fn test_extract_message_content_ignores_non_text_blocks() {
        let content = serde_json::json!([
            {"type": "tool_use", "name": "Read"},
            {"type": "thinking", "text": "hidden"}
        ]);
        assert_eq!(extract_message_content(Some(&content)), None);
    }
}
