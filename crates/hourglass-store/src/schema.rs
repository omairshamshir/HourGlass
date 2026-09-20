//! Database schema and migrations.
//!
//! Timestamps are Unix seconds (INTEGER): sortable, comparable, and free of
//! parsing on every read, which keeps the day's queries in the microseconds.

use rusqlite::Connection;

/// Bump this when adding a migration below.
pub const SCHEMA_VERSION: i32 = 1;

/// Apply every migration the database has not seen yet.
pub fn migrate(conn: &Connection) -> rusqlite::Result<()> {
    // WAL keeps the UI's reads from ever blocking on the writer.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;

    let current: i32 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if current >= SCHEMA_VERSION {
        return Ok(());
    }

    if current < 1 {
        conn.execute_batch(V1)?;
    }

    conn.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

const V1: &str = r#"
CREATE TABLE projects (
    id         INTEGER PRIMARY KEY AUTOINCREMENT,
    name       TEXT    NOT NULL,
    color      INTEGER NOT NULL DEFAULT 0,
    archived   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX idx_projects_name ON projects(name COLLATE NOCASE);

CREATE TABLE time_entries (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id  INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    started_at  INTEGER NOT NULL,
    ended_at    INTEGER,
    stop_reason TEXT,
    CHECK (ended_at IS NULL OR ended_at >= started_at)
);

CREATE INDEX idx_entries_started ON time_entries(started_at);
CREATE INDEX idx_entries_project ON time_entries(project_id);

-- At most one entry may be open at a time. Every open row indexes the same
-- value, so the unique constraint rejects a second one.
CREATE UNIQUE INDEX idx_entries_single_open
    ON time_entries((ended_at IS NULL)) WHERE ended_at IS NULL;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#;
