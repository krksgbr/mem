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

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub id: Option<String>,
    pub participant: Participant,
    pub content: String,
    pub timestamp: Option<i64>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct Conversation {
    pub id: String,
    pub external_id: Option<String>,
    pub title: Option<String>,
    pub provider: ProviderKind,
    pub created_at: i64,
    pub updated_at: i64,
    pub messages: Vec<Message>,
}

impl Conversation {
    pub fn display_title(&self) -> String {
        if let Some(title) = &self.title {
            if !title.trim().is_empty() {
                return title.clone();
            }
        }

        self.messages
            .iter()
            .find_map(|message| {
                let first_line = message.content.lines().next()?.trim();
                if first_line.is_empty() {
                    None
                } else if first_line.len() > 60 {
                    Some(format!("{}...", &first_line[..57]))
                } else {
                    Some(first_line.to_string())
                }
            })
            .unwrap_or_else(|| self.id.clone())
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
