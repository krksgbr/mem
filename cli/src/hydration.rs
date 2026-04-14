use crate::{indexed, providers};
use anyhow::{anyhow, Result};
use shared::{visible_conversation_target, ConversationLoadRef, Model};

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

    anyhow!(
        "failed to hydrate indexed conversation: indexed_id='{indexed_conversation_id}', conversation_id='{conversation_id}', external_id='{}', load_ref={load_ref_summary}",
        external_id.unwrap_or("<none>")
    )
}
