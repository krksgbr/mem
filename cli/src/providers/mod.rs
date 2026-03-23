pub mod claude;
pub mod codex;

use anyhow::Result;
use shared::{Conversation, ProviderKind};

pub fn hydrate_conversation(conversation: &Conversation) -> Result<Option<Conversation>> {
    match conversation.provider {
        ProviderKind::ClaudeCode => claude::hydrate_conversation(conversation),
        ProviderKind::Codex => codex::hydrate_conversation(conversation),
    }
}
