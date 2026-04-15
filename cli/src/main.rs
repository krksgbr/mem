use anyhow::Result;
use args::{Command, RunArgs};
use chrono::Utc;
use crossterm::event::{self, Event as CrosstermEvent};
use crux_core::App;
use profile_scroll::{InteractivePhaseDurations, InteractiveProfiler};
use ratatui::widgets::{ListState, TableState};
use shared::{Event, Model, TranscriptBrowser, ViewModel};
use std::path::PathBuf;
use std::time::Instant;

mod args;
mod dump_screen;
mod hydration;
mod indexed;
mod input;
mod profile_scroll;
mod providers;
mod render;
mod runtime;
mod screen_ref;
mod storage;
mod theme;

#[cfg(test)]
mod test_utils;

#[cfg(test)]
mod test_compare;
#[cfg(test)]
mod test_large;
#[cfg(test)]
mod test_textwrap;

#[tokio::main]
async fn main() -> Result<()> {
    match args::parse_args(std::env::args())? {
        Command::Run(args) => run_interactive(args).await,
        Command::DumpScreen(args) => dump_screen_command(&args).await,
        Command::ProfileScroll(args) => profile_scroll_command(&args).await,
        Command::Search(args) => search_command(&args).await,
        Command::Read(args) => read_command(&args).await,
    }
}

async fn run_interactive(args: RunArgs) -> Result<()> {
    let report_path = args.profile.then(interactive_profile_path).transpose()?;
    let mut profiler = args.profile.then_some(InteractiveProfiler::new(0, 0));
    if let Some(profiler) = profiler.as_mut() {
        profiler.log_event("startup", "starting interactive run");
        if let Some(path) = report_path.as_ref() {
            profiler.persist(path, "starting", None)?;
        }
    }

    storage::ensure_default_index()?;
    if let Some(profiler) = profiler.as_mut() {
        profiler.log_event("startup", "ensured default index");
        if let Some(path) = report_path.as_ref() {
            profiler.persist(path, "starting", None)?;
        }
    }
    if indexed::load_workspace_summaries()?.is_empty() {
        if let Some(profiler) = profiler.as_mut() {
            profiler.log_event(
                "startup",
                "workspace summaries empty, running foreground sync",
            );
            if let Some(path) = report_path.as_ref() {
                profiler.persist(path, "starting", None)?;
            }
        }
        let _ = indexed::sync_now()?;
        if let Some(profiler) = profiler.as_mut() {
            profiler.log_event("startup", "foreground sync completed");
            if let Some(path) = report_path.as_ref() {
                profiler.persist(path, "starting", None)?;
            }
        }
    }
    let workspaces = indexed::load_workspace_summaries()?;
    let now_ms = Utc::now().timestamp_millis();

    let app = TranscriptBrowser;
    let mut model = Model::default();
    let _ = app.update(Event::SetWorkspaces(workspaces, now_ms), &mut model, &());

    let mut session = runtime::TerminalSession::enter()?;
    let mut list_state = ListState::default();
    let mut table_state = TableState::default();
    let theme = theme::Theme::default();
    let sync_rx = indexed::spawn_background_sync();
    let terminal_size = session.terminal_mut().size()?;
    if let Some(profiler) = profiler.as_mut() {
        profiler.set_dimensions(terminal_size.width, terminal_size.height);
        profiler.log_event(
            "startup",
            format!(
                "entered terminal session at {}x{} with {} workspaces",
                terminal_size.width,
                terminal_size.height,
                model.workspaces.len()
            ),
        );
        if let Some(path) = report_path.as_ref() {
            profiler.persist(path, "running", None)?;
        }
    }

    let loop_result: Result<()> = loop {
        let frame_start = Instant::now();
        if let Ok(result) = sync_rx.try_recv() {
            if let Some(updated_workspaces) = result? {
                let now_ms = Utc::now().timestamp_millis();
                let _ = app.update(
                    Event::SetWorkspaces(updated_workspaces, now_ms),
                    &mut model,
                    &(),
                );
                if let Some(profiler) = profiler.as_mut() {
                    profiler.log_event(
                        "background_sync",
                        format!(
                            "applied updated workspace snapshot ({} workspaces)",
                            model.workspaces.len()
                        ),
                    );
                    if let Some(path) = report_path.as_ref() {
                        profiler.persist(path, "running", None)?;
                    }
                }
            }
        }

        let hydrate_start = Instant::now();
        hydration::hydrate_visible_conversation(&mut model)?;
        let hydrate_ms = elapsed_ms(hydrate_start);

        let view_start = Instant::now();
        let view_model: ViewModel = app.view(&model);
        let view_ms = elapsed_ms(view_start);
        list_state.select(Some(view_model.selected_index));
        table_state.select(Some(view_model.selected_index));

        let render_start = Instant::now();
        session.terminal_mut().draw(|f| {
            render::render_ui(f, &view_model, &mut list_state, &mut table_state, &theme);
        })?;
        let render_ms = elapsed_ms(render_start);

        let poll_start = Instant::now();
        let mut key_code = None;
        let mut update_ms = 0.0;
        let mut input_outcome_name = None;
        let mut had_interaction = false;
        if event::poll(std::time::Duration::from_millis(16))? {
            if let CrosstermEvent::Key(key) = event::read()? {
                had_interaction = true;
                key_code = Some(key.code);
                let outcome = input::handle_key_code(&view_model, key.code)?;
                input_outcome_name = Some(input_outcome_label(&outcome));
                if matches!(outcome, input::InputOutcome::Quit) {
                    if let Some(profiler) = profiler.as_mut() {
                        profiler.log_event("input", format!("quit via key {:?}", key.code));
                        profiler.record_frame(
                            &view_model,
                            terminal_area_chars(terminal_size.width, terminal_size.height),
                            key_code,
                            input_outcome_name,
                            InteractivePhaseDurations {
                                poll_wait: elapsed_ms(poll_start),
                                update: update_ms,
                                hydrate: hydrate_ms,
                                view: view_ms,
                                render: render_ms,
                                total: elapsed_ms(frame_start),
                            },
                        );
                        if let Some(path) = report_path.as_ref() {
                            profiler.persist(path, "completed", None)?;
                        }
                    }
                    break Ok(());
                }
                match outcome {
                    input::InputOutcome::Continue | input::InputOutcome::Quit => {}
                    input::InputOutcome::Event(event) => {
                        let event_label = format!("{event:?}");
                        let update_start = Instant::now();
                        let _ = app.update(event, &mut model, &());
                        update_ms = elapsed_ms(update_start);
                        if let Some(profiler) = profiler.as_mut() {
                            profiler.log_event(
                                "input",
                                format!("applied event {event_label} from key {:?}", key.code),
                            );
                        }
                    }
                    input::InputOutcome::CopyActiveId => {
                        let id = view_model.active_id.as_ref().ok_or_else(|| {
                            anyhow::anyhow!("copy requested but no active conversation is selected")
                        })?;
                        let mut clipboard = arboard::Clipboard::new()?;
                        clipboard.set_text(id.clone())?;
                        model.status_text = Some(format!("Copied active id: {id}"));
                        if let Some(profiler) = profiler.as_mut() {
                            profiler.log_event("input", format!("copied active id {id}"));
                        }
                    }
                    input::InputOutcome::CopyScreenRef => {
                        let screen_ref = screen_ref::capture_screen_ref(
                            &model,
                            &view_model,
                            terminal_size.width,
                            terminal_size.height,
                        )?;
                        let path = interactive_screen_ref_path()?;
                        screen_ref::write_screen_ref(&path, &screen_ref)?;

                        let mut clipboard = arboard::Clipboard::new()?;
                        clipboard.set_text(path.display().to_string())?;
                        model.status_text = Some(format!("Saved screen ref: {}", path.display()));
                        if let Some(profiler) = profiler.as_mut() {
                            profiler.log_event(
                                "input",
                                format!("captured screen ref to {}", path.display()),
                            );
                        }
                    }
                }
            }
        }

        if had_interaction {
            if let Some(profiler) = profiler.as_mut() {
                profiler.record_frame(
                    &view_model,
                    terminal_area_chars(terminal_size.width, terminal_size.height),
                    key_code,
                    input_outcome_name,
                    InteractivePhaseDurations {
                        poll_wait: elapsed_ms(poll_start),
                        update: update_ms,
                        hydrate: hydrate_ms,
                        view: view_ms,
                        render: render_ms,
                        total: elapsed_ms(frame_start),
                    },
                );
                if let Some(path) = report_path.as_ref() {
                    profiler.persist(path, "running", None)?;
                }
            }
        }
    };

    session.restore()?;
    if let Some(profiler) = profiler {
        if let Some(path) = report_path.as_ref() {
            match &loop_result {
                Ok(()) => profiler.persist(path, "completed", None)?,
                Err(error) => profiler.persist(path, "failed", Some(&error.to_string()))?,
            }
            println!("wrote profile report to {}", path.display());
        }
    }

    loop_result
}

async fn dump_screen_command(args: &args::DumpScreenArgs) -> Result<()> {
    ensure_read_only_index_ready()?;
    let workspaces = indexed::load_workspace_summaries()?;
    let now_ms = Utc::now().timestamp_millis();
    let output = dump_screen::dump_screen_output(args, workspaces, now_ms)?;
    print!("{output}");
    Ok(())
}

async fn search_command(args: &args::SearchArgs) -> Result<()> {
    ensure_read_only_index_ready()?;
    let results = indexed::search(&args.query, args.limit)?;
    println!("{}", serde_json::to_string_pretty(&results)?);
    Ok(())
}

async fn read_command(args: &args::ReadArgs) -> Result<()> {
    storage::ensure_default_index()?;
    let result = indexed::read(&args.conversation, args.offset, args.limit)?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn profile_scroll_command(args: &args::ProfileScrollArgs) -> Result<()> {
    ensure_read_only_index_ready()?;
    let workspaces = indexed::load_workspace_summaries()?;
    let now_ms = Utc::now().timestamp_millis();
    let output = profile_scroll::profile_scroll_output(args, workspaces, now_ms)?;
    println!("{output}");
    Ok(())
}

fn ensure_read_only_index_ready() -> Result<()> {
    storage::ensure_default_index()?;
    if indexed::load_workspace_summaries()?.is_empty() {
        let _ = indexed::sync_now()?;
    }
    Ok(())
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn terminal_area_chars(width: u16, height: u16) -> usize {
    usize::from(width) * usize::from(height)
}

fn interactive_profile_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join("transcript-browser-profile.json"))
}

fn interactive_screen_ref_path() -> Result<PathBuf> {
    Ok(std::env::current_dir()?.join("transcript-browser-screen-ref.json"))
}

fn input_outcome_label(outcome: &input::InputOutcome) -> &'static str {
    match outcome {
        input::InputOutcome::Continue => "continue",
        input::InputOutcome::Quit => "quit",
        input::InputOutcome::Event(_) => "event",
        input::InputOutcome::CopyActiveId => "copy_active_id",
        input::InputOutcome::CopyScreenRef => "copy_screen_ref",
    }
}
