use crate::args::{DumpScreenArgs, ProfileScrollArgs, ScreenTarget, ScrollDirection};
use crate::{dump_screen, hydration};
use anyhow::Result;
use crossterm::event::KeyCode;
use crux_core::App;
use serde::Serialize;
use shared::{Event, Model, TranscriptBrowser, ViewModel, Workspace};
use std::fs;
use std::path::Path;
use std::time::Instant;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Serialize)]
pub struct ScrollProfileReport {
    pub workspace: String,
    pub conversation: String,
    pub steps: usize,
    pub direction: String,
    pub width: u16,
    pub height: u16,
    pub initial_message_index: usize,
    pub initial_render_chars: usize,
    pub totals_ms: PhaseDurations,
    pub averages_ms: PhaseDurations,
    pub worst_step: StepTiming,
    pub steps_profiled: Vec<StepTiming>,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct PhaseDurations {
    pub update: f64,
    pub hydrate: f64,
    pub view: f64,
    pub render: f64,
    pub total: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct StepTiming {
    pub step: usize,
    pub selected_index: usize,
    pub render_chars: usize,
    pub durations_ms: PhaseDurations,
}

#[derive(Debug, Clone, Serialize)]
pub struct InteractiveProfileReport {
    pub status: String,
    pub error: Option<String>,
    pub started_at_ms: i64,
    pub finished_at_ms: Option<i64>,
    pub frames: usize,
    pub key_frames: usize,
    pub width: u16,
    pub height: u16,
    pub log: Vec<InteractiveLogEvent>,
    pub totals_ms: InteractivePhaseDurations,
    pub averages_ms: InteractivePhaseDurations,
    pub worst_frame: InteractiveFrameTiming,
    pub frames_profiled: Vec<InteractiveFrameTiming>,
}

#[derive(Debug, Clone, Serialize)]
pub struct InteractiveFrameTiming {
    pub frame: usize,
    pub title: String,
    pub breadcrumb: String,
    pub filter_text: String,
    pub active_id: Option<String>,
    pub selected_index: usize,
    pub terminal_cells: usize,
    pub key_code: Option<String>,
    pub input_outcome: Option<String>,
    pub durations_ms: InteractivePhaseDurations,
}

#[derive(Debug, Clone, Serialize)]
pub struct InteractiveLogEvent {
    pub seq: usize,
    pub timestamp_ms: i64,
    pub phase: String,
    pub detail: String,
}

#[derive(Debug, Clone, Copy, Serialize, Default)]
pub struct InteractivePhaseDurations {
    pub poll_wait: f64,
    pub update: f64,
    pub hydrate: f64,
    pub view: f64,
    pub render: f64,
    pub total: f64,
}

#[derive(Debug, Default)]
pub struct InteractiveProfiler {
    width: u16,
    height: u16,
    started_at_ms: i64,
    frames: Vec<InteractiveFrameTiming>,
    log: Vec<InteractiveLogEvent>,
    totals: InteractivePhaseDurations,
    key_frames: usize,
}

impl InteractiveProfiler {
    pub fn new(width: u16, height: u16) -> Self {
        Self {
            width,
            height,
            started_at_ms: now_ms(),
            ..Self::default()
        }
    }

    pub fn log_event(&mut self, phase: impl Into<String>, detail: impl Into<String>) {
        self.log.push(InteractiveLogEvent {
            seq: self.log.len(),
            timestamp_ms: now_ms(),
            phase: phase.into(),
            detail: detail.into(),
        });
    }

    pub fn set_dimensions(&mut self, width: u16, height: u16) {
        self.width = width;
        self.height = height;
    }

    pub fn record_frame(
        &mut self,
        view_model: &ViewModel,
        terminal_cells: usize,
        key_code: Option<KeyCode>,
        input_outcome: Option<&str>,
        durations_ms: InteractivePhaseDurations,
    ) {
        if key_code.is_some() {
            self.key_frames += 1;
        }

        self.totals.poll_wait += durations_ms.poll_wait;
        self.totals.update += durations_ms.update;
        self.totals.hydrate += durations_ms.hydrate;
        self.totals.view += durations_ms.view;
        self.totals.render += durations_ms.render;
        self.totals.total += durations_ms.total;

        self.frames.push(InteractiveFrameTiming {
            frame: self.frames.len(),
            title: view_model.title.clone(),
            breadcrumb: view_model.breadcrumb.clone(),
            filter_text: view_model.filter_text.clone(),
            active_id: view_model.active_id.clone(),
            selected_index: view_model.selected_index,
            terminal_cells,
            key_code: key_code.map(key_code_label),
            input_outcome: input_outcome.map(ToOwned::to_owned),
            durations_ms,
        });
    }

    pub fn persist(&self, path: &Path, status: &str, error: Option<&str>) -> Result<()> {
        let json = serde_json::to_string_pretty(&self.snapshot(status, error))?;
        fs::write(path, json)?;
        Ok(())
    }

    pub fn snapshot(&self, status: &str, error: Option<&str>) -> InteractiveProfileReport {
        let frame_count = self.frames.len().max(1) as f64;
        let averages_ms = InteractivePhaseDurations {
            poll_wait: self.totals.poll_wait / frame_count,
            update: self.totals.update / frame_count,
            hydrate: self.totals.hydrate / frame_count,
            view: self.totals.view / frame_count,
            render: self.totals.render / frame_count,
            total: self.totals.total / frame_count,
        };
        let worst_frame = self
            .frames
            .iter()
            .max_by(|left, right| left.durations_ms.total.total_cmp(&right.durations_ms.total))
            .cloned()
            .unwrap_or(InteractiveFrameTiming {
                frame: 0,
                title: String::new(),
                breadcrumb: String::new(),
                filter_text: String::new(),
                active_id: None,
                selected_index: 0,
                terminal_cells: 0,
                key_code: None,
                input_outcome: None,
                durations_ms: InteractivePhaseDurations::default(),
            });

        InteractiveProfileReport {
            status: status.to_string(),
            error: error.map(ToOwned::to_owned),
            started_at_ms: self.started_at_ms,
            finished_at_ms: Some(now_ms()),
            frames: self.frames.len(),
            key_frames: self.key_frames,
            width: self.width,
            height: self.height,
            log: self.log.clone(),
            totals_ms: self.totals,
            averages_ms,
            worst_frame,
            frames_profiled: self.frames.clone(),
        }
    }
}

pub fn profile_scroll_output(
    args: &ProfileScrollArgs,
    workspaces: Vec<Workspace>,
    default_now_ms: i64,
) -> Result<String> {
    let report = profile_scroll(args, workspaces, default_now_ms)?;
    Ok(serde_json::to_string_pretty(&report)?)
}

fn profile_scroll(
    args: &ProfileScrollArgs,
    workspaces: Vec<Workspace>,
    default_now_ms: i64,
) -> Result<ScrollProfileReport> {
    let now_ms = args.now_ms.unwrap_or(default_now_ms);
    let mut model = Model::default();
    let app = TranscriptBrowser;

    let _ = app.update(Event::SetWorkspaces(workspaces, now_ms), &mut model, &());
    dump_screen::resolve_screen(
        &app,
        &mut model,
        &DumpScreenArgs {
            screen_ref: None,
            screen: ScreenTarget::Messages,
            workspace: Some(args.workspace.clone()),
            conversation: Some(args.conversation.clone()),
            provider: args.provider,
            layout: None,
            width: args.width,
            height: args.height,
            now_ms: Some(now_ms),
            selected: 0,
            message_index: args.message_index,
            expand_all: false,
        },
    )?;

    hydration::hydrate_visible_conversation(&mut model)?;
    let initial_view = app.view(&model);
    let initial_render =
        dump_screen::render_view_model_to_string(&initial_view, args.width, args.height)?;

    let mut steps_profiled = Vec::with_capacity(args.steps);
    let mut totals = PhaseDurations::default();

    for step in 0..args.steps {
        let total_start = Instant::now();

        let update_start = Instant::now();
        let event = match args.direction {
            ScrollDirection::Down => Event::MessageDown,
            ScrollDirection::Up => Event::MessageUp,
        };
        let _ = app.update(event, &mut model, &());
        let update_ms = elapsed_ms(update_start);

        let hydrate_start = Instant::now();
        hydration::hydrate_visible_conversation(&mut model)?;
        let hydrate_ms = elapsed_ms(hydrate_start);

        let view_start = Instant::now();
        let view_model: ViewModel = app.view(&model);
        let view_ms = elapsed_ms(view_start);

        let render_start = Instant::now();
        let rendered =
            dump_screen::render_view_model_to_string(&view_model, args.width, args.height)?;
        let render_ms = elapsed_ms(render_start);

        let total_ms = elapsed_ms(total_start);
        let timing = StepTiming {
            step,
            selected_index: view_model.selected_index,
            render_chars: rendered.len(),
            durations_ms: PhaseDurations {
                update: update_ms,
                hydrate: hydrate_ms,
                view: view_ms,
                render: render_ms,
                total: total_ms,
            },
        };

        totals.update += update_ms;
        totals.hydrate += hydrate_ms;
        totals.view += view_ms;
        totals.render += render_ms;
        totals.total += total_ms;
        steps_profiled.push(timing);
    }

    let step_count = steps_profiled.len().max(1) as f64;
    let averages = PhaseDurations {
        update: totals.update / step_count,
        hydrate: totals.hydrate / step_count,
        view: totals.view / step_count,
        render: totals.render / step_count,
        total: totals.total / step_count,
    };
    let worst_step = steps_profiled
        .iter()
        .max_by(|left, right| left.durations_ms.total.total_cmp(&right.durations_ms.total))
        .cloned()
        .unwrap_or(StepTiming {
            step: 0,
            selected_index: initial_view.selected_index,
            render_chars: initial_render.len(),
            durations_ms: PhaseDurations::default(),
        });

    Ok(ScrollProfileReport {
        workspace: args.workspace.clone(),
        conversation: args.conversation.clone(),
        steps: args.steps,
        direction: match args.direction {
            ScrollDirection::Down => "down".into(),
            ScrollDirection::Up => "up".into(),
        },
        width: args.width,
        height: args.height,
        initial_message_index: args.message_index,
        initial_render_chars: initial_render.len(),
        totals_ms: totals,
        averages_ms: averages,
        worst_step,
        steps_profiled,
    })
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

fn key_code_label(code: KeyCode) -> String {
    match code {
        KeyCode::Backspace => "Backspace".into(),
        KeyCode::Enter => "Enter".into(),
        KeyCode::Left => "Left".into(),
        KeyCode::Right => "Right".into(),
        KeyCode::Up => "Up".into(),
        KeyCode::Down => "Down".into(),
        KeyCode::Home => "Home".into(),
        KeyCode::End => "End".into(),
        KeyCode::PageUp => "PageUp".into(),
        KeyCode::PageDown => "PageDown".into(),
        KeyCode::Tab => "Tab".into(),
        KeyCode::BackTab => "BackTab".into(),
        KeyCode::Delete => "Delete".into(),
        KeyCode::Insert => "Insert".into(),
        KeyCode::Esc => "Esc".into(),
        KeyCode::Char(ch) => ch.to_string(),
        KeyCode::F(number) => format!("F{number}"),
        KeyCode::Null => "Null".into(),
        KeyCode::CapsLock => "CapsLock".into(),
        KeyCode::ScrollLock => "ScrollLock".into(),
        KeyCode::NumLock => "NumLock".into(),
        KeyCode::PrintScreen => "PrintScreen".into(),
        KeyCode::Pause => "Pause".into(),
        KeyCode::Menu => "Menu".into(),
        KeyCode::KeypadBegin => "KeypadBegin".into(),
        KeyCode::Media(media) => format!("Media::{media:?}"),
        KeyCode::Modifier(modifier) => format!("Modifier::{modifier:?}"),
    }
}
