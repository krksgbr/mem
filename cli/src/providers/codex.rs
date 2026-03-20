use anyhow::Result;
use chrono::DateTime;
use serde::Deserialize;
use serde_json::Value;
use shared::{Conversation, Message, Participant, ProviderKind, Workspace};
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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

pub fn load_workspaces() -> Result<Vec<Workspace>> {
    let mut by_path: BTreeMap<String, Workspace> = BTreeMap::new();
    let home_dir = std::env::var("HOME")?;
    let base_path = PathBuf::from(&home_dir).join(".codex/sessions");

    if !base_path.exists() {
        return Ok(Vec::new());
    }

    for file_path in discover_session_files(&base_path)? {
        if let Some((workspace_path, conversation)) = parse_session_file(&file_path)? {
            let workspace_key = workspace_path.unwrap_or_else(|| ".".to_string());
            let display_name = prettify_path(&workspace_key, &home_dir);
            let workspace = by_path
                .entry(workspace_key.clone())
                .or_insert_with(|| Workspace {
                    id: workspace_key.clone(),
                    display_name,
                    source_path: if workspace_key == "." {
                        None
                    } else {
                        Some(workspace_key.clone())
                    },
                    updated_at: 0,
                    conversations: Vec::new(),
                });
            workspace.updated_at = workspace.updated_at.max(conversation.updated_at);
            workspace.conversations.push(conversation);
        }
    }

    let mut workspaces: Vec<Workspace> = by_path.into_values().collect();
    for workspace in &mut workspaces {
        workspace
            .conversations
            .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    }
    workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(workspaces)
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

fn parse_session_file(file_path: &Path) -> Result<Option<(Option<String>, Conversation)>> {
    let content = fs::read_to_string(file_path)?;
    let file_stem = match file_path.file_stem().and_then(|stem| stem.to_str()) {
        Some(stem) => stem.to_string(),
        None => return Ok(None),
    };

    let mut external_id = None;
    let mut cwd = None;
    let mut messages = Vec::new();
    let mut created_at = i64::MAX;
    let mut updated_at = 0i64;

    for line in content.lines() {
        let Ok(entry) = serde_json::from_str::<CodexLine>(line) else {
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
                let Some(message) = parse_response_item(item, timestamp_ms) else {
                    continue;
                };
                if let Some(ts) = message.timestamp {
                    created_at = created_at.min(ts);
                    updated_at = updated_at.max(ts);
                }
                messages.push(message);
            }
            _ => {}
        }
    }

    if messages.is_empty() {
        return Ok(None);
    }

    if created_at == i64::MAX {
        created_at = updated_at;
    }

    let external_id = external_id.unwrap_or_else(|| file_stem.clone());
    let conversation_id = external_id.chars().take(8).collect::<String>();
    let title = load_title_from_session_index(&external_id)?;

    Ok(Some((
        cwd.clone(),
        Conversation {
            id: conversation_id,
            external_id: Some(external_id),
            title,
            provider: ProviderKind::Codex,
            created_at,
            updated_at,
            messages,
        },
    )))
}

fn parse_response_item(item: ResponseItemPayload, timestamp_ms: Option<i64>) -> Option<Message> {
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
    Some(Message {
        id: None,
        participant,
        content,
        timestamp: timestamp_ms,
    })
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

#[cfg(test)]
mod tests {
    use super::*;

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
        let message = parse_response_item(item, Some(123)).expect("message");
        assert_eq!(message.participant, Participant::System);
        assert_eq!(message.timestamp, Some(123));
    }
}
