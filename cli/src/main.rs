use anyhow::Result;
use args::{Command, LatestArgs, RunArgs, WorkspacesArgs};
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
mod trial_trace;

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
        Command::Workspaces(args) => workspaces_command(&args).await,
        Command::Latest(args) => latest_command(&args).await,
        Command::Search(args) => search_command(&args).await,
        Command::Read(args) => read_command(&args).await,
        Command::Help(text) => {
            println!("{text}");
            Ok(())
        }
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
        if let Err(error) = hydration::hydrate_visible_conversation(&mut model) {
            let now_ms = Utc::now().timestamp_millis();
            if hydration::recover_missing_indexed_conversation(&app, &mut model, now_ms, &error)? {
                if let Some(profiler) = profiler.as_mut() {
                    profiler.log_event(
                        "hydration_recovery",
                        format!("recovered from stale indexed conversation: {error}"),
                    );
                    if let Some(path) = report_path.as_ref() {
                        profiler.persist(path, "running", None)?;
                    }
                }
            } else {
                return Err(error);
            }
        }
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
        let mut poll_wait_ms = 0.0;
        let mut update_ms = 0.0;
        let mut input_outcome_name = None;
        let mut had_interaction = false;
        if event::poll(std::time::Duration::from_millis(16))? {
            poll_wait_ms = elapsed_ms(poll_start);
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
                                poll_wait: poll_wait_ms,
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
                        poll_wait: poll_wait_ms,
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
    trial_trace::record_search(&args.query, args.limit, &results)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        print!("{}", format_search_results(&results));
    }
    Ok(())
}

async fn read_command(args: &args::ReadArgs) -> Result<()> {
    storage::ensure_default_index()?;
    let result = indexed::read(&args.conversation, args.offset, args.limit)?;
    trial_trace::record_read(&args.conversation, args.offset, args.limit, result.as_ref())?;
    let result = result.ok_or_else(|| {
        anyhow::anyhow!(
            "conversation '{}' not found. Pass an internal conversation id from search output, a provider external id, or an exact title.",
            args.conversation
        )
    })?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&result)?);
    } else {
        print!("{}", format_read_result(&result));
    }
    Ok(())
}

async fn workspaces_command(args: &WorkspacesArgs) -> Result<()> {
    ensure_read_only_index_ready()?;
    let results = indexed::list_workspaces(args.provider)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        print!("{}", format_workspaces(&results));
    }
    Ok(())
}

async fn latest_command(args: &LatestArgs) -> Result<()> {
    ensure_read_only_index_ready()?;
    let results =
        indexed::latest_conversations(args.provider, args.workspace.as_deref(), args.limit)?;
    if args.json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else {
        print!("{}", format_latest_results(&results));
    }
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

fn provider_display(provider: &str) -> &str {
    match provider {
        "claude_code" => "Claude Code",
        "codex" => "Codex",
        other => other,
    }
}

fn format_search_results(results: &[indexed::SearchResult]) -> String {
    if results.is_empty() {
        return "No conversations found.\n".to_string();
    }

    let mut out = String::new();
    for (idx, result) in results.iter().enumerate() {
        out.push_str(&format!("{}. Title: {}\n", idx + 1, result.title));
        out.push_str(&format!(
            "   Agent: {}\n",
            provider_display(&result.provider),
        ));
        out.push_str(&format!("   Workspace: {}\n", result.workspace));
        out.push_str(&format!(
            "   Last active: {}\n",
            relative_time(result.updated_at_ms)
        ));
        out.push_str(&format!("   Matched text: {}\n", result.snippet.trim()));
        out.push_str(&format!("   Conversation ID: {}\n", result.conversation_id));
        if idx + 1 != results.len() {
            out.push('\n');
        }
    }
    out
}

fn format_latest_results(results: &[indexed::RecentConversationResult]) -> String {
    if results.is_empty() {
        return "No conversations found.\n".to_string();
    }

    let mut out = String::new();
    for (idx, result) in results.iter().enumerate() {
        out.push_str(&format!("{}. Title: {}\n", idx + 1, result.title));
        out.push_str(&format!(
            "   Agent: {}\n",
            provider_display(&result.provider),
        ));
        out.push_str(&format!("   Workspace: {}\n", result.workspace));
        out.push_str(&format!(
            "   Last active: {}\n",
            relative_time(result.updated_at_ms)
        ));
        out.push_str(&format!("   Latest message: {}\n", result.snippet.trim()));
        out.push_str(&format!("   Conversation ID: {}\n", result.conversation_id));
        if idx + 1 != results.len() {
            out.push('\n');
        }
    }
    out
}

fn format_workspaces(results: &[indexed::WorkspaceListResult]) -> String {
    if results.is_empty() {
        return "No workspaces found.\n".to_string();
    }

    let mut out = String::new();
    for (idx, result) in results.iter().enumerate() {
        out.push_str(&format!("{}. Workspace: {}\n", idx + 1, result.display_name));
        if let Some(path) = &result.canonical_path {
            out.push_str(&format!("   Path: {}\n", path));
        }
        out.push_str(&format!(
            "   Conversations: {} total • Claude Code: {} • Codex: {}\n",
            result.conversation_count,
            result.claude_code_conversation_count,
            result.codex_conversation_count,
        ));
        out.push_str(&format!(
            "   Last active: {}\n",
            relative_time(result.updated_at_ms)
        ));
        out.push_str(&format!("   Workspace ID: {}\n", result.workspace_id));
        if idx + 1 != results.len() {
            out.push('\n');
        }
    }
    out
}

fn format_read_result(result: &indexed::ReadResult) -> String {
    let mut out = String::new();
    out.push_str(&format!("Title: {}\n", result.title));
    out.push_str(&format!(
        "Agent: {}\n",
        provider_display(&result.provider),
    ));
    out.push_str(&format!("Conversation ID: {}\n", result.conversation_id));
    out.push_str(&format!("Entries in conversation: {}\n", result.total_entries));
    out.push_str(&format!("Read offset: {}\n", result.offset));
    out.push_str(&format!("Read limit: {}\n\n", result.limit));

    for (idx, entry) in result.entries.iter().enumerate() {
        let timestamp = entry
            .timestamp_ms
            .map(relative_time)
            .unwrap_or_else(|| "unknown time".to_string());
        out.push_str(&format!(
            "{}. Participant: {} • Kind: {} • Time: {}\n",
            result.offset + idx + 1,
            entry.participant,
            entry.kind,
            timestamp
        ));
        for line in entry.content.lines() {
            out.push_str(&format!("   {}\n", line));
        }
        out.push('\n');
    }

    if let Some(next_offset) = result.next_offset {
        out.push_str(&format!("next offset: {}\n", next_offset));
    }
    out
}

fn relative_time(timestamp_ms: i64) -> String {
    let now_ms = Utc::now().timestamp_millis();
    let delta_ms = now_ms.saturating_sub(timestamp_ms).max(0);
    let minute = 60_000;
    let hour = 60 * minute;
    let day = 24 * hour;

    if delta_ms < hour {
        format!("{}m ago", (delta_ms / minute).max(1))
    } else if delta_ms < day {
        format!("{}h ago", (delta_ms / hour).max(1))
    } else {
        format!("{}d ago", (delta_ms / day).max(1))
    }
}
