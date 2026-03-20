use anyhow::{Context, Result};
use crossterm::event::KeyCode;
use crux_core::Core;
use shared::{Event, TranscriptBrowser, ViewModel};

pub enum InputOutcome {
    Continue,
    Quit,
}

pub fn handle_key_code(
    core: &Core<TranscriptBrowser>,
    view_model: &ViewModel,
    key_code: KeyCode,
) -> Result<InputOutcome> {
    match key_code {
        KeyCode::Char('q') | KeyCode::Char('Q') => Ok(InputOutcome::Quit),
        KeyCode::Up | KeyCode::Char('k') => {
            core.process_event(Event::Up);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Down | KeyCode::Char('j') => {
            core.process_event(Event::Down);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Char('K') => {
            core.process_event(Event::MessageUp);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Char('J') => {
            core.process_event(Event::MessageDown);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Enter | KeyCode::Right | KeyCode::Char('l') => {
            core.process_event(Event::Select);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Esc | KeyCode::Backspace | KeyCode::Left | KeyCode::Char('h') => {
            core.process_event(Event::Back);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Char('f') | KeyCode::Char('F') => {
            core.process_event(Event::CycleFilter);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Char('e') | KeyCode::Char('E') => {
            core.process_event(Event::ToggleMessage);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Char('~') => {
            core.process_event(Event::ToggleLayout);
            Ok(InputOutcome::Continue)
        }
        KeyCode::Char('Y') | KeyCode::Char('y') => {
            let id = view_model
                .active_id
                .as_ref()
                .context("copy requested but no active conversation is selected")?;
            let mut clipboard =
                arboard::Clipboard::new().context("failed to initialize clipboard")?;
            clipboard
                .set_text(id.clone())
                .context("failed to copy conversation id to clipboard")?;
            Ok(InputOutcome::Continue)
        }
        _ => Ok(InputOutcome::Continue),
    }
}
