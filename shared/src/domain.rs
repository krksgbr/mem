use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ProviderKind {
    ClaudeCode,
    Codex,
}

impl std::fmt::Display for ProviderKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ProviderKind::ClaudeCode => write!(f, "Claude Code"),
            ProviderKind::Codex => write!(f, "Codex"),
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum Participant {
    User,
    Assistant { provider: ProviderKind },
    System,
    Tool { name: Option<String> },
    Unknown { raw_role: String },
}

impl Participant {
    pub fn from_role(role: &str, provider: ProviderKind) -> Self {
        match role.to_ascii_lowercase().as_str() {
            "user" => Self::User,
            "assistant" => Self::Assistant { provider },
            "system" => Self::System,
            "tool" => Self::Tool { name: None },
            other => Self::Unknown {
                raw_role: other.to_string(),
            },
        }
    }

    pub fn label(&self) -> String {
        match self {
            Participant::User => "You".to_string(),
            Participant::Assistant { provider } => provider.to_string(),
            Participant::System => "System".to_string(),
            Participant::Tool { name: Some(name) } => name.clone(),
            Participant::Tool { name: None } => "Tool".to_string(),
            Participant::Unknown { raw_role } => raw_role.clone(),
        }
    }

    pub fn is_user(&self) -> bool {
        matches!(self, Participant::User)
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MessageKind {
    UserMessage,
    AssistantMessage,
    ToolCall,
    ToolResult,
    Thinking,
    Summary,
    Compaction,
    Label,
    MetadataChange,
}

impl MessageKind {
    pub fn is_searchable_by_default(self) -> bool {
        !matches!(self, Self::Thinking)
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: Option<String>,
    pub kind: MessageKind,
    pub participant: Participant,
    pub content: String,
    pub timestamp: Option<i64>,
    pub parent_id: Option<String>,
    pub associated_id: Option<String>,
    pub depth: usize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub enum ConversationLoadRef {
    ClaudeFile { path: String },
    CodexFiles { paths: Vec<String> },
    Indexed { conversation_id: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub external_id: Option<String>,
    pub branch_parent_id: Option<String>,
    pub branch_anchor_message_id: Option<String>,
    pub title: Option<String>,
    pub preview: Option<String>,
    pub provider: ProviderKind,
    pub created_at: i64,
    pub updated_at: i64,
    pub segments: Vec<ConversationSegment>,
    pub messages: Vec<Message>,
    pub is_hydrated: bool,
    pub load_ref: Option<ConversationLoadRef>,
}

impl Conversation {
    pub fn has_segments(&self) -> bool {
        self.segments.len() > 1
    }

    pub fn has_loaded_messages(&self) -> bool {
        self.is_hydrated
    }

    pub fn preview_line(&self) -> Option<&str> {
        self.messages
            .iter()
            .find(|message| message.kind == MessageKind::UserMessage)
            .and_then(first_non_empty_line)
            .or_else(|| self.messages.iter().find_map(first_non_empty_line))
            .or(self.preview.as_deref())
    }

    pub fn display_title(&self) -> String {
        if let Some(title) = &self.title {
            if !title.trim().is_empty() {
                return title.clone();
            }
        }

        self.preview_line()
            .map(truncate_preview_line)
            .unwrap_or_else(|| self.id.clone())
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct ConversationSegment {
    pub id: String,
    pub label: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub message_start_idx: usize,
    pub message_count: usize,
}

fn first_non_empty_line(message: &Message) -> Option<&str> {
    message
        .content
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
}

fn truncate_preview_line(line: &str) -> String {
    if line.len() > 60 {
        format!("{}...", &line[..57])
    } else {
        line.to_string()
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Workspace {
    pub id: String,
    pub display_name: String,
    pub source_path: Option<String>,
    pub updated_at: i64,
    pub conversations: Vec<Conversation>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message(participant: Participant, content: &str) -> Message {
        Message {
            id: None,
            kind: match participant {
                Participant::User => MessageKind::UserMessage,
                Participant::Assistant { .. } => MessageKind::AssistantMessage,
                Participant::Tool { .. } => MessageKind::ToolResult,
                Participant::System | Participant::Unknown { .. } => MessageKind::MetadataChange,
            },
            participant,
            content: content.into(),
            timestamp: None,
            parent_id: None,
            associated_id: None,
            depth: 0,
        }
    }

    #[test]
    fn display_title_prefers_first_user_message() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::Codex,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![
                message(Participant::System, "<collaboration_mode># Collaboration"),
                message(Participant::User, "Actual user prompt"),
            ],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.display_title(), "Actual user prompt");
        assert_eq!(conversation.preview_line(), Some("Actual user prompt"));
    }

    #[test]
    fn display_title_falls_back_when_no_user_message_exists() {
        let conversation = Conversation {
            id: "conv-1".into(),
            external_id: None,
            branch_parent_id: None,
            branch_anchor_message_id: None,
            title: None,
            preview: None,
            provider: ProviderKind::ClaudeCode,
            created_at: 0,
            updated_at: 0,
            segments: vec![],
            messages: vec![message(Participant::System, "Only system content")],
            is_hydrated: true,
            load_ref: None,
        };

        assert_eq!(conversation.display_title(), "Only system content");
        assert_eq!(conversation.preview_line(), Some("Only system content"));
    }
}
