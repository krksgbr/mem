use anyhow::{Context, Result};
use rusqlite::Connection;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SCHEMA_VERSION: i32 = 21;
const PROD_STATE_NAMESPACE: &str = "transcript-browser";
const DEV_STATE_NAMESPACE: &str = "transcript-browser-dev";

const SCHEMA_STATEMENTS: &[&str] = &[
    r#"
    CREATE TABLE IF NOT EXISTS source_file (
        id TEXT PRIMARY KEY,
        provider TEXT NOT NULL,
        path TEXT NOT NULL UNIQUE,
        status TEXT NOT NULL,
        workspace_hint TEXT,
        file_size_bytes INTEGER,
        mtime_ms INTEGER,
        content_fingerprint TEXT,
        last_indexed_at_ms INTEGER,
        last_seen_at_ms INTEGER,
        parse_error TEXT,
        provider_metadata_json TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS workspace (
        id TEXT PRIMARY KEY,
        canonical_path TEXT,
        display_name TEXT NOT NULL,
        provider_scope TEXT,
        status TEXT NOT NULL DEFAULT 'active',
        created_at_ms INTEGER,
        updated_at_ms INTEGER,
        metadata_json TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS conversation (
        id TEXT PRIMARY KEY,
        workspace_id TEXT REFERENCES workspace(id),
        provider TEXT NOT NULL,
        provider_conversation_id TEXT,
        parent_conversation_id TEXT REFERENCES conversation(id),
        title TEXT,
        preview_text TEXT,
        status TEXT NOT NULL DEFAULT 'active',
        created_at_ms INTEGER,
        updated_at_ms INTEGER,
        last_source_event_at_ms INTEGER,
        metadata_json TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS conversation_source_file (
        conversation_id TEXT NOT NULL REFERENCES conversation(id),
        source_file_id TEXT NOT NULL REFERENCES source_file(id),
        role TEXT,
        ordinal INTEGER,
        metadata_json TEXT,
        PRIMARY KEY (conversation_id, source_file_id)
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS conversation_entry (
        id TEXT PRIMARY KEY,
        conversation_id TEXT NOT NULL REFERENCES conversation(id),
        kind TEXT NOT NULL,
        parent_entry_id TEXT REFERENCES conversation_entry(id),
        associated_entry_id TEXT REFERENCES conversation_entry(id),
        source_file_id TEXT REFERENCES source_file(id),
        provider_entry_id TEXT,
        ordinal INTEGER NOT NULL,
        timestamp_ms INTEGER,
        is_searchable INTEGER NOT NULL DEFAULT 1,
        search_text TEXT,
        summary_text TEXT,
        metadata_json TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS entry_block (
        id TEXT PRIMARY KEY,
        entry_id TEXT NOT NULL REFERENCES conversation_entry(id),
        ordinal INTEGER NOT NULL,
        kind TEXT NOT NULL,
        text_value TEXT,
        json_value TEXT,
        mime_type TEXT,
        metadata_json TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS entry_label (
        entry_id TEXT PRIMARY KEY REFERENCES conversation_entry(id),
        label TEXT NOT NULL,
        color TEXT,
        metadata_json TEXT
    )
    "#,
    r#"
    CREATE TABLE IF NOT EXISTS artifact_lexicon (
        term TEXT PRIMARY KEY,
        doc_freq INTEGER NOT NULL
    )
    "#,
    r#"
    CREATE VIRTUAL TABLE IF NOT EXISTS conversation_fts USING fts5(
        entry_id UNINDEXED,
        conversation_id UNINDEXED,
        title_text,
        preview_text,
        opening_prompt_text,
        early_user_context_text,
        artifact_text,
        search_text,
        tokenize='unicode61'
    )
    "#,
    r#"
    CREATE VIRTUAL TABLE IF NOT EXISTS conversation_fts_nl USING fts5(
        entry_id UNINDEXED,
        conversation_id UNINDEXED,
        title_text,
        preview_text,
        opening_prompt_text,
        early_user_context_text,
        artifact_text,
        search_text,
        tokenize='porter unicode61'
    )
    "#,
    "CREATE INDEX IF NOT EXISTS idx_conversation_workspace_updated ON conversation(workspace_id, updated_at_ms DESC)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_provider_updated ON conversation(provider, updated_at_ms DESC)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_provider_external ON conversation(provider, provider_conversation_id)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_parent ON conversation(parent_conversation_id)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_source_file_source ON conversation_source_file(source_file_id)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_entry_conversation_ordinal ON conversation_entry(conversation_id, ordinal)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_entry_conversation_timestamp ON conversation_entry(conversation_id, timestamp_ms)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_entry_parent ON conversation_entry(parent_entry_id)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_entry_associated ON conversation_entry(associated_entry_id)",
    "CREATE INDEX IF NOT EXISTS idx_conversation_entry_provider_entry ON conversation_entry(provider_entry_id)",
    "CREATE INDEX IF NOT EXISTS idx_entry_block_entry_ordinal ON entry_block(entry_id, ordinal)",
];

const DROP_STATEMENTS: &[&str] = &[
    "DROP TABLE IF EXISTS conversation_fts",
    "DROP TABLE IF EXISTS conversation_fts_nl",
    "DROP TABLE IF EXISTS artifact_lexicon",
    "DROP TABLE IF EXISTS entry_label",
    "DROP TABLE IF EXISTS entry_block",
    "DROP TABLE IF EXISTS conversation_entry",
    "DROP TABLE IF EXISTS conversation_source_file",
    "DROP TABLE IF EXISTS conversation",
    "DROP TABLE IF EXISTS workspace",
    "DROP TABLE IF EXISTS source_file",
];

pub struct Storage {
    connection: Connection,
}

impl Storage {
    pub fn open_default() -> Result<Self> {
        let path = default_index_path()?;
        ensure_parent_dir(&path)?;
        Self::open(path)
    }

    pub fn open<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let connection = Connection::open(path.as_ref()).with_context(|| {
            format!("failed to open SQLite index at {}", path.as_ref().display())
        })?;
        Self::from_connection(connection)
    }

    #[cfg(test)]
    pub fn open_in_memory() -> Result<Self> {
        let connection =
            Connection::open_in_memory().context("failed to open in-memory SQLite index")?;
        Self::from_connection(connection)
    }

    pub fn schema_version(&self) -> Result<i32> {
        self.connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .context("failed to read SQLite user_version")
    }

    pub fn rebuild(&mut self) -> Result<()> {
        let tx = self
            .connection
            .transaction()
            .context("failed to start SQLite rebuild transaction")?;

        for statement in DROP_STATEMENTS {
            tx.execute(statement, [])
                .with_context(|| format!("failed to execute schema drop statement: {statement}"))?;
        }

        apply_schema_to_connection(&tx)?;
        tx.commit()
            .context("failed to commit SQLite rebuild transaction")?;
        Ok(())
    }

    pub fn raw_connection(&self) -> &Connection {
        &self.connection
    }

    pub fn raw_connection_mut(&mut self) -> &mut Connection {
        &mut self.connection
    }

    #[cfg(test)]
    pub fn connection(&self) -> &Connection {
        &self.connection
    }

    fn from_connection(connection: Connection) -> Result<Self> {
        configure_connection(&connection)?;
        let mut storage = Self { connection };
        storage.prepare_schema()?;
        Ok(storage)
    }

    fn prepare_schema(&mut self) -> Result<()> {
        match self.schema_version()? {
            0 => apply_schema_to_connection(&self.connection),
            SCHEMA_VERSION => Ok(()),
            _ => self.rebuild(),
        }
    }
}

fn configure_connection(connection: &Connection) -> Result<()> {
    connection
        .busy_timeout(Duration::from_secs(30))
        .context("failed to configure SQLite busy timeout")?;
    connection
        .execute_batch(
            r#"
            PRAGMA foreign_keys = ON;
            PRAGMA journal_mode = WAL;
            PRAGMA synchronous = NORMAL;
            PRAGMA temp_store = MEMORY;
            "#,
        )
        .context("failed to configure SQLite pragmas")?;
    Ok(())
}

fn apply_schema_to_connection(connection: &Connection) -> Result<()> {
    for statement in SCHEMA_STATEMENTS {
        connection
            .execute(statement, [])
            .with_context(|| format!("failed to execute SQLite schema statement: {statement}"))?;
    }

    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .context("failed to set SQLite schema version")?;
    Ok(())
}

pub fn ensure_default_index() -> Result<()> {
    let path = default_index_path()?;
    ensure_parent_dir(&path)?;
    let _ = Storage::open(&path)?;
    Ok(())
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .context("default SQLite index path has no parent directory")?;

    fs::create_dir_all(parent).with_context(|| {
        format!(
            "failed to create SQLite index directory at {}",
            parent.display()
        )
    })?;

    Ok(())
}

pub fn default_state_dir() -> Result<PathBuf> {
    default_state_dir_from_env_and_exe(
        env::var("XDG_STATE_HOME").ok(),
        env::var("HOME").ok(),
        env::current_exe().ok(),
    )
}

fn default_index_path() -> Result<PathBuf> {
    Ok(default_state_dir()?.join("index.sqlite3"))
}

fn default_state_dir_from_env_and_exe(
    xdg_state_home: Option<String>,
    home: Option<String>,
    current_exe: Option<PathBuf>,
) -> Result<PathBuf> {
    let namespace = state_namespace_for_current_exe(current_exe.as_deref());

    if let Some(state_home) = xdg_state_home {
        return Ok(PathBuf::from(state_home).join(namespace));
    }

    let home = home.context(
        "failed to determine default transcript-browser state directory: neither XDG_STATE_HOME nor HOME is set",
    )?;

    Ok(PathBuf::from(home)
        .join(".local")
        .join("state")
        .join(namespace))
}

fn state_namespace_for_current_exe(current_exe: Option<&Path>) -> &'static str {
    if current_exe.is_some_and(is_repo_target_executable) {
        DEV_STATE_NAMESPACE
    } else {
        PROD_STATE_NAMESPACE
    }
}

fn is_repo_target_executable(current_exe: &Path) -> bool {
    current_exe.starts_with(repo_target_root())
}

fn repo_target_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("cli crate should live under the workspace root")
        .join("target")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::OptionalExtension;

    fn table_exists(storage: &Storage, table_name: &str) -> bool {
        storage
            .connection()
            .query_row(
                "SELECT name FROM sqlite_master WHERE type IN ('table', 'view') AND name = ?1",
                [table_name],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap()
            .is_some()
    }

    fn row_count(storage: &Storage, table_name: &str) -> i64 {
        storage
            .connection()
            .query_row(&format!("SELECT COUNT(*) FROM {table_name}"), [], |row| {
                row.get(0)
            })
            .unwrap()
    }

    #[test]
    fn opens_with_expected_schema() {
        let storage = Storage::open_in_memory().unwrap();

        assert_eq!(storage.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(table_exists(&storage, "source_file"));
        assert!(table_exists(&storage, "workspace"));
        assert!(table_exists(&storage, "conversation"));
        assert!(table_exists(&storage, "conversation_source_file"));
        assert!(table_exists(&storage, "conversation_entry"));
        assert!(table_exists(&storage, "entry_block"));
        assert!(table_exists(&storage, "entry_label"));
        assert!(table_exists(&storage, "artifact_lexicon"));
        assert!(table_exists(&storage, "conversation_fts"));
        assert!(table_exists(&storage, "conversation_fts_nl"));
    }

    #[test]
    fn rebuild_resets_schema_contents() {
        let mut storage = Storage::open_in_memory().unwrap();

        storage
            .connection()
            .execute(
                "INSERT INTO source_file (id, provider, path, status) VALUES (?1, ?2, ?3, ?4)",
                ("source-1", "claude", "/tmp/example.jsonl", "active"),
            )
            .unwrap();
        storage
            .connection()
            .execute(
                "INSERT INTO workspace (id, display_name) VALUES (?1, ?2)",
                ("workspace-1", "example"),
            )
            .unwrap();

        assert_eq!(row_count(&storage, "source_file"), 1);
        assert_eq!(row_count(&storage, "workspace"), 1);

        storage.rebuild().unwrap();

        assert_eq!(storage.schema_version().unwrap(), SCHEMA_VERSION);
        assert!(table_exists(&storage, "artifact_lexicon"));
        assert!(table_exists(&storage, "conversation_fts"));
        assert!(table_exists(&storage, "conversation_fts_nl"));
        assert_eq!(row_count(&storage, "source_file"), 0);
        assert_eq!(row_count(&storage, "workspace"), 0);
    }

    #[test]
    fn default_index_path_prefers_xdg_state_home() {
        let path =
            default_state_dir_from_env_and_exe(
                Some("/tmp/xdg-state".into()),
                Some("/tmp/home".into()),
                None,
            )
            .unwrap()
            .join("index.sqlite3");

        assert_eq!(
            path,
            PathBuf::from("/tmp/xdg-state/transcript-browser/index.sqlite3")
        );
    }

    #[test]
    fn default_index_path_falls_back_to_home() {
        let path = default_state_dir_from_env_and_exe(None, Some("/tmp/home".into()), None)
            .unwrap()
            .join("index.sqlite3");

        assert_eq!(
            path,
            PathBuf::from("/tmp/home/.local/state/transcript-browser/index.sqlite3")
        );
    }

    #[test]
    fn default_state_dir_prefers_xdg_state_home() {
        let path =
            default_state_dir_from_env_and_exe(
                Some("/tmp/xdg-state".into()),
                Some("/tmp/home".into()),
                None,
            )
            .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/xdg-state/transcript-browser"));
    }

    #[test]
    fn default_state_dir_falls_back_to_home() {
        let path = default_state_dir_from_env_and_exe(None, Some("/tmp/home".into()), None).unwrap();

        assert_eq!(
            path,
            PathBuf::from("/tmp/home/.local/state/transcript-browser")
        );
    }

    #[test]
    fn repo_target_executable_uses_dev_state_namespace() {
        let exe = repo_target_root().join("debug").join("mem");
        let path = default_state_dir_from_env_and_exe(
            Some("/tmp/xdg-state".into()),
            Some("/tmp/home".into()),
            Some(exe),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/xdg-state/transcript-browser-dev"));
    }

    #[test]
    fn installed_executable_keeps_prod_state_namespace() {
        let exe = PathBuf::from("/tmp/home/.local/bin/mem");
        let path = default_state_dir_from_env_and_exe(
            Some("/tmp/xdg-state".into()),
            Some("/tmp/home".into()),
            Some(exe),
        )
        .unwrap();

        assert_eq!(path, PathBuf::from("/tmp/xdg-state/transcript-browser"));
    }
}
