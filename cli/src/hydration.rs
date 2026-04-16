use crate::{indexed, providers};
use anyhow::{anyhow, Result};
use crux_core::App;
use shared::{visible_conversation_target, ConversationLoadRef, Event, Model, TranscriptBrowser};
use std::error::Error;
use std::fmt::{self, Display, Formatter};

#[derive(Debug)]
struct MissingIndexedConversationError {
    indexed_conversation_id: String,
    conversation_id: String,
    external_id: Option<String>,
    load_ref_summary: String,
}

impl Display for MissingIndexedConversationError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "failed to hydrate indexed conversation: indexed_id='{}', conversation_id='{}', external_id='{}', load_ref={}",
            self.indexed_conversation_id,
            self.conversation_id,
            self.external_id.as_deref().unwrap_or("<none>"),
            self.load_ref_summary
        )
    }
}

impl Error for MissingIndexedConversationError {}

pub fn hydrate_visible_conversation(model: &mut Model) -> Result<()> {
    let Some((workspace_idx, conv_idx)) = visible_conversation_target(model) else {
        return Ok(());
    };

    hydrate_conversation(model, workspace_idx, conv_idx)
}

pub fn hydrate_conversation(
    model: &mut Model,
    workspace_idx: usize,
    conv_idx: usize,
) -> Result<()> {
    let Some(workspace) = model.workspaces.get_mut(workspace_idx) else {
        return Err(anyhow!("workspace index {} is out of range", workspace_idx));
    };
    let Some(conversation) = workspace.conversations.get(conv_idx) else {
        return Err(anyhow!("conversation index {} is out of range", conv_idx));
    };
    if conversation.has_loaded_messages() {
        return Ok(());
    }

    let Some(load_ref) = conversation.load_ref.as_ref() else {
        return Ok(());
    };

    let conversation_id = match load_ref {
        shared::ConversationLoadRef::Indexed { conversation_id } => conversation_id,
        shared::ConversationLoadRef::ClaudeFile { .. }
        | shared::ConversationLoadRef::CodexFiles { .. } => {
            let hydrated = providers::hydrate_conversation(conversation)?.ok_or_else(|| {
                anyhow!(
                    "failed to hydrate conversation '{}'",
                    conversation
                        .external_id
                        .as_deref()
                        .unwrap_or(&conversation.id)
                )
            })?;
            workspace.conversations[conv_idx] = hydrated;
            return Ok(());
        }
    };

    let hydrated = indexed::hydrate_conversation(conversation_id)?.ok_or_else(|| {
        indexed_hydration_error(
            conversation_id,
            conversation.load_ref.as_ref(),
            &conversation.id,
            conversation.external_id.as_deref(),
        )
    })?;
    workspace.conversations[conv_idx] = hydrated;
    Ok(())
}

pub fn recover_missing_indexed_conversation(
    app: &TranscriptBrowser,
    model: &mut Model,
    current_time_ms: i64,
    error: &anyhow::Error,
) -> Result<bool> {
    if error
        .downcast_ref::<MissingIndexedConversationError>()
        .is_none()
    {
        return Ok(false);
    }

    let workspaces = indexed::load_workspace_summaries()?;
    let _ = app.update(
        Event::SetWorkspaces(workspaces, current_time_ms),
        model,
        &(),
    );
    model.status_text =
        Some("Conversation disappeared during index refresh. Reloaded current view.".to_string());
    Ok(true)
}

fn indexed_hydration_error(
    indexed_conversation_id: &str,
    load_ref: Option<&ConversationLoadRef>,
    conversation_id: &str,
    external_id: Option<&str>,
) -> anyhow::Error {
    let load_ref_summary = match load_ref {
        Some(ConversationLoadRef::Indexed { conversation_id }) => {
            format!("Indexed({conversation_id})")
        }
        Some(ConversationLoadRef::ClaudeFile { path }) => format!("ClaudeFile({path})"),
        Some(ConversationLoadRef::CodexFiles { paths }) => {
            format!("CodexFiles({} paths)", paths.len())
        }
        None => "None".to_string(),
    };

    MissingIndexedConversationError {
        indexed_conversation_id: indexed_conversation_id.to_string(),
        conversation_id: conversation_id.to_string(),
        external_id: external_id.map(ToOwned::to_owned),
        load_ref_summary,
    }
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::{Conversation, ProviderKind, Screen, Workspace};

    #[test]
    fn recover_missing_indexed_conversation_ignores_other_errors() {
        let app = TranscriptBrowser;
        let mut model = Model::default();

        let recovered = recover_missing_indexed_conversation(
            &app,
            &mut model,
            123,
            &anyhow!("some other failure"),
        )
        .expect("recovery check should not fail");

        assert!(!recovered);
        assert_eq!(model.status_text, None);
    }

    #[test]
    fn recover_missing_indexed_conversation_refreshes_model_from_index() {
        let app = TranscriptBrowser;
        let mut model = Model {
            workspaces: vec![Workspace {
                id: "stale".into(),
                display_name: "Stale".into(),
                source_path: Some("/tmp/stale".into()),
                updated_at: 1,
                conversations: vec![Conversation {
                    id: "conv".into(),
                    external_id: Some("conv".into()),
                    branch_parent_id: None,
                    branch_anchor_message_id: None,
                    title: Some("old".into()),
                    preview: None,
                    provider: ProviderKind::ClaudeCode,
                    created_at: 1,
                    updated_at: 1,
                    segments: Vec::new(),
                    messages: Vec::new(),
                    is_hydrated: false,
                    load_ref: Some(ConversationLoadRef::Indexed {
                        conversation_id: "conv".into(),
                    }),
                }],
            }],
            screen: Screen::Workspaces {
                selected_workspace: 0,
            },
            provider_filter: None,
            current_time: 0,
            status_text: None,
        };

        let err = indexed_hydration_error(
            "conv",
            Some(&ConversationLoadRef::Indexed {
                conversation_id: "conv".into(),
            }),
            "conv",
            Some("conv"),
        );
        let recovered = recover_missing_indexed_conversation(&app, &mut model, 123, &err)
            .expect("recovery should succeed");

        assert!(recovered);
        assert_eq!(
            model.status_text.as_deref(),
            Some("Conversation disappeared during index refresh. Reloaded current view.")
        );
    }
}
