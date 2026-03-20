use anyhow::Result;
use chrono::Utc;
use crossterm::event::{self, Event as CrosstermEvent};
use crux_core::Core;
use ratatui::widgets::{ListState, TableState};
use shared::{Event, TranscriptBrowser, ViewModel};

mod input;
mod providers;
mod render;
mod runtime;
mod test_utils;
mod theme;

#[cfg(test)]
mod test_compare;
#[cfg(test)]
mod test_large;
#[cfg(test)]
mod test_textwrap;

#[tokio::main]
async fn main() -> Result<()> {
    let workspaces = providers::load_all_workspaces()?;
    let now_ms = Utc::now().timestamp_millis();

    let core: Core<TranscriptBrowser> = Core::default();
    core.process_event(Event::SetWorkspaces(workspaces, now_ms));

    let mut session = runtime::TerminalSession::enter()?;
    let mut list_state = ListState::default();
    let mut table_state = TableState::default();
    let theme = theme::Theme::default();

    loop {
        let view_model: ViewModel = core.view();
        list_state.select(Some(view_model.selected_index));
        table_state.select(Some(view_model.selected_index));

        session.terminal_mut().draw(|f| {
            render::render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
        })?;

        if event::poll(std::time::Duration::from_millis(16))? {
            if let CrosstermEvent::Key(key) = event::read()? {
                let outcome = input::handle_key_code(&core, &view_model, key.code)?;
                if matches!(outcome, input::InputOutcome::Quit) {
                    break;
                }
            }
        }
    }

    session.restore()?;
    Ok(())
}
