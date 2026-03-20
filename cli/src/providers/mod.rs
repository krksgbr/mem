pub mod claude;
pub mod codex;

use anyhow::Result;
use shared::Workspace;
use std::collections::BTreeMap;

pub fn load_all_workspaces() -> Result<Vec<Workspace>> {
    let mut merged: BTreeMap<String, Workspace> = BTreeMap::new();

    for workspace in claude::load_workspaces()? {
        merge_workspace(&mut merged, workspace);
    }
    for workspace in codex::load_workspaces()? {
        merge_workspace(&mut merged, workspace);
    }

    let mut workspaces: Vec<Workspace> = merged.into_values().collect();
    for workspace in &mut workspaces {
        workspace
            .conversations
            .sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    }
    workspaces.sort_by(|left, right| right.updated_at.cmp(&left.updated_at));
    Ok(workspaces)
}

fn merge_workspace(merged: &mut BTreeMap<String, Workspace>, workspace: Workspace) {
    let key = workspace
        .source_path
        .clone()
        .unwrap_or_else(|| workspace.display_name.clone());

    match merged.get_mut(&key) {
        Some(existing) => {
            existing.updated_at = existing.updated_at.max(workspace.updated_at);
            existing.conversations.extend(workspace.conversations);
            if existing.source_path.is_none() {
                existing.source_path = workspace.source_path;
            }
        }
        None => {
            merged.insert(key, workspace);
        }
    }
}
