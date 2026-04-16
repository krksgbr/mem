use crate::indexed::{ReadResult, SearchResult};
use anyhow::Result;

#[cfg(feature = "trial-tracing")]
use {
    crate::storage,
    anyhow::Context,
    chrono::{TimeZone, Utc},
    serde::{Deserialize, Serialize},
    std::fs::{self, OpenOptions},
    std::io::Write,
    std::path::{Path, PathBuf},
};

#[cfg(feature = "trial-tracing")]
const TRACE_IDLE_ROTATE_AFTER_MS: i64 = 5 * 60 * 1000;
#[cfg(feature = "trial-tracing")]
const TRACE_RUN_STATE_FILE: &str = "current-run.json";
#[cfg(feature = "trial-tracing")]
const TRACE_RUNS_DIR: &str = "trials";

#[cfg(feature = "trial-tracing")]
#[derive(Debug, Serialize)]
struct TrialTraceSearchHit<'a> {
    rank: usize,
    conversation_id: &'a str,
    title: &'a str,
    snippet: &'a str,
}

#[cfg(feature = "trial-tracing")]
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TrialTraceEvent<'a> {
    Search {
        ts: String,
        pid: u32,
        query: &'a str,
        limit: usize,
        result_count: usize,
        results: Vec<TrialTraceSearchHit<'a>>,
    },
    Read {
        ts: String,
        pid: u32,
        conversation_id: &'a str,
        offset: usize,
        limit: usize,
        found: bool,
        entry_count: usize,
        returned_conversation_id: Option<&'a str>,
    },
}

#[cfg(feature = "trial-tracing")]
#[derive(Debug, Serialize, Deserialize)]
struct TrialTraceRunState {
    path: String,
    last_event_at_ms: i64,
}

#[cfg(feature = "trial-tracing")]
pub fn record_search(query: &str, limit: usize, results: &[SearchResult]) -> Result<()> {
    let now = Utc::now();
    let event = TrialTraceEvent::Search {
        ts: now.to_rfc3339(),
        pid: std::process::id(),
        query,
        limit,
        result_count: results.len(),
        results: results
            .iter()
            .enumerate()
            .map(|(index, result)| TrialTraceSearchHit {
                rank: index + 1,
                conversation_id: result.conversation_id.as_str(),
                title: result.title.as_str(),
                snippet: result.snippet.as_str(),
            })
            .collect(),
    };
    append_event(default_trial_trace_root()?, now.timestamp_millis(), &event)
}

#[cfg(not(feature = "trial-tracing"))]
pub fn record_search(_query: &str, _limit: usize, _results: &[SearchResult]) -> Result<()> {
    Ok(())
}

#[cfg(feature = "trial-tracing")]
pub fn record_read(
    conversation_id: &str,
    offset: usize,
    limit: usize,
    result: Option<&ReadResult>,
) -> Result<()> {
    let now = Utc::now();
    let event = TrialTraceEvent::Read {
        ts: now.to_rfc3339(),
        pid: std::process::id(),
        conversation_id,
        offset,
        limit,
        found: result.is_some(),
        entry_count: result.map(|value| value.entries.len()).unwrap_or(0),
        returned_conversation_id: result.map(|value| value.conversation_id.as_str()),
    };
    append_event(default_trial_trace_root()?, now.timestamp_millis(), &event)
}

#[cfg(not(feature = "trial-tracing"))]
pub fn record_read(
    _conversation_id: &str,
    _offset: usize,
    _limit: usize,
    _result: Option<&ReadResult>,
) -> Result<()> {
    Ok(())
}

#[cfg(feature = "trial-tracing")]
fn default_trial_trace_root() -> Result<PathBuf> {
    Ok(storage::default_state_dir()?.join(TRACE_RUNS_DIR))
}

#[cfg(feature = "trial-tracing")]
fn append_event(root: PathBuf, now_ms: i64, event: &TrialTraceEvent<'_>) -> Result<()> {
    fs::create_dir_all(&root).with_context(|| {
        format!(
            "failed to create transcript-browser trial trace directory at {}",
            root.display()
        )
    })?;

    let path = resolve_trial_trace_path(&root, now_ms, std::process::id())?;
    append_event_to_path(&path, event)?;
    write_run_state(
        &root,
        &TrialTraceRunState {
            path: path.display().to_string(),
            last_event_at_ms: now_ms,
        },
    )?;
    Ok(())
}

#[cfg(feature = "trial-tracing")]
fn resolve_trial_trace_path(root: &Path, now_ms: i64, pid: u32) -> Result<PathBuf> {
    if let Some(state) = load_run_state(root)? {
        let path = PathBuf::from(&state.path);
        let is_recent = now_ms.saturating_sub(state.last_event_at_ms) <= TRACE_IDLE_ROTATE_AFTER_MS;
        if is_recent {
            return Ok(path);
        }
    }

    Ok(root.join(format!(
        "trial-{}-{}.jsonl",
        format_trace_timestamp(now_ms)?,
        pid
    )))
}

#[cfg(feature = "trial-tracing")]
fn format_trace_timestamp(now_ms: i64) -> Result<String> {
    let dt = Utc
        .timestamp_millis_opt(now_ms)
        .single()
        .context("failed to convert trace timestamp to UTC datetime")?;
    Ok(dt.format("%Y-%m-%dT%H-%M-%SZ").to_string())
}

#[cfg(feature = "trial-tracing")]
fn run_state_path(root: &Path) -> PathBuf {
    root.join(TRACE_RUN_STATE_FILE)
}

#[cfg(feature = "trial-tracing")]
fn load_run_state(root: &Path) -> Result<Option<TrialTraceRunState>> {
    let path = run_state_path(root);
    if !path.exists() {
        return Ok(None);
    }

    let body = fs::read_to_string(&path)
        .with_context(|| format!("failed to read trial trace run state at {}", path.display()))?;
    let state = serde_json::from_str(&body).with_context(|| {
        format!(
            "failed to parse trial trace run state at {}",
            path.display()
        )
    })?;
    Ok(Some(state))
}

#[cfg(feature = "trial-tracing")]
fn write_run_state(root: &Path, state: &TrialTraceRunState) -> Result<()> {
    let path = run_state_path(root);
    let body = serde_json::to_string(state).context("failed to serialize trial trace run state")?;
    fs::write(&path, body).with_context(|| {
        format!(
            "failed to write trial trace run state to {}",
            path.display()
        )
    })?;
    Ok(())
}

#[cfg(feature = "trial-tracing")]
fn append_event_to_path(path: &Path, event: &TrialTraceEvent<'_>) -> Result<()> {
    let serialized =
        serde_json::to_string(event).context("failed to serialize trial trace event")?;
    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("failed to open trial trace artifact at {}", path.display()))?;
    writeln!(file, "{serialized}")
        .with_context(|| format!("failed to append trial trace event to {}", path.display()))?;
    Ok(())
}

#[cfg(all(test, feature = "trial-tracing"))]
mod tests {
    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "transcript-browser-trial-traces-{label}-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        root
    }

    #[test]
    fn appends_search_event_as_jsonl() {
        let root = temp_root("search");
        let path = root.join("trial.jsonl");
        let event = TrialTraceEvent::Search {
            ts: "2026-04-16T11:30:00Z".into(),
            pid: 123,
            query: "lightdash",
            limit: 5,
            result_count: 2,
            results: vec![
                TrialTraceSearchHit {
                    rank: 1,
                    conversation_id: "conv-1",
                    title: "Lightdash setup",
                    snippet: "deploy lightdash on GCP",
                },
                TrialTraceSearchHit {
                    rank: 2,
                    conversation_id: "conv-2",
                    title: "Lightdash access follow-up",
                    snippet: "revoke blanket access",
                },
            ],
        };

        append_event_to_path(&path, &event).unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"kind\":\"search\""));
        assert!(body.contains("\"query\":\"lightdash\""));
        assert!(body.contains("\"rank\":1"));
        assert!(body.contains("\"rank\":2"));
        assert!(body.contains("\"title\":\"Lightdash setup\""));
        assert!(body.contains("\"snippet\":\"deploy lightdash on GCP\""));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn appends_read_event_as_jsonl() {
        let root = temp_root("read");
        let path = root.join("trial.jsonl");
        let event = TrialTraceEvent::Read {
            ts: "2026-04-16T11:31:00Z".into(),
            pid: 123,
            conversation_id: "conv-1",
            offset: 0,
            limit: 20,
            found: true,
            entry_count: 3,
            returned_conversation_id: Some("conv-1"),
        };

        append_event_to_path(&path, &event).unwrap();

        let body = fs::read_to_string(&path).unwrap();
        assert!(body.contains("\"kind\":\"read\""));
        assert!(body.contains("\"conversation_id\":\"conv-1\""));
        assert!(body.contains("\"entry_count\":3"));
        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn reuses_recent_run_file_and_rotates_after_idle_threshold() {
        let root = temp_root("rotation");
        let first_path = resolve_trial_trace_path(&root, 1_000, 111).unwrap();
        write_run_state(
            &root,
            &TrialTraceRunState {
                path: first_path.display().to_string(),
                last_event_at_ms: 1_000,
            },
        )
        .unwrap();

        let reused =
            resolve_trial_trace_path(&root, 1_000 + TRACE_IDLE_ROTATE_AFTER_MS - 1, 222).unwrap();
        assert_eq!(reused, first_path);

        let rotated =
            resolve_trial_trace_path(&root, 1_000 + TRACE_IDLE_ROTATE_AFTER_MS + 1, 333).unwrap();
        assert_ne!(rotated, first_path);
        assert!(rotated
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("333"));
        let _ = fs::remove_dir_all(root);
    }
}
