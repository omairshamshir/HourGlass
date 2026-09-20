//! SQLite persistence for projects, entries, and settings.
//!
//! The store is deliberately dumb: it records what the timer decided. The one
//! rule it enforces on its own is that at most one entry can be open, because
//! that invariant must survive a crash, not just a well-behaved caller.

mod schema;

use chrono::{DateTime, TimeZone, Utc};
use hourglass_core::model::{
    EntryId, Project, ProjectId, Settings, StopReason, ThemeChoice, TimeEntry,
};
use hourglass_core::report::DateRange;
use hourglass_core::timer::Session;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};

/// Anything that can go wrong talking to the database.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("could not create the data directory: {0}")]
    Io(#[from] std::io::Error),
    #[error("a project named \"{0}\" already exists")]
    DuplicateProject(String),
    #[error("a timer is already running")]
    AlreadyRunning,
    #[error("project name cannot be empty")]
    EmptyProjectName,
    #[error("an entry cannot end before it starts")]
    BackwardsEntry,
}

pub type Result<T> = std::result::Result<T, StoreError>;

/// An open database.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open (and migrate) the database at `path`, creating parent folders.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        schema::migrate(&conn)?;
        Ok(Store { conn })
    }

    /// An empty in-memory database, used by tests.
    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        schema::migrate(&conn)?;
        Ok(Store { conn })
    }

    /// `~/Library/Application Support/Hourglass/hourglass.db`, unless
    /// `HOURGLASS_DB` names somewhere else. The override exists so a scratch
    /// database can be used without touching real recorded hours.
    pub fn default_path() -> PathBuf {
        if let Some(override_path) = std::env::var_os("HOURGLASS_DB") {
            return PathBuf::from(override_path);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join("Library/Application Support/Hourglass/hourglass.db")
    }

    // -- projects ---------------------------------------------------------

    /// Every project, active ones first, each group alphabetical.
    pub fn projects(&self) -> Result<Vec<Project>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, name, color, archived, created_at
             FROM projects
             ORDER BY archived ASC, name COLLATE NOCASE ASC",
        )?;
        let rows = stmt.query_map([], row_to_project)?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Add a project. Names are unique regardless of case.
    pub fn create_project(&self, name: &str, color: u8) -> Result<Project> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::EmptyProjectName);
        }
        let created_at = Utc::now();
        let result = self.conn.execute(
            "INSERT INTO projects (name, color, archived, created_at) VALUES (?1, ?2, 0, ?3)",
            params![name, i64::from(color), created_at.timestamp()],
        );

        match result {
            Ok(_) => Ok(Project {
                id: ProjectId(self.conn.last_insert_rowid()),
                name: name.to_string(),
                color,
                archived: false,
                created_at: from_unix(created_at.timestamp()),
            }),
            Err(err) if is_unique_violation(&err) => {
                Err(StoreError::DuplicateProject(name.to_string()))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn rename_project(&self, id: ProjectId, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() {
            return Err(StoreError::EmptyProjectName);
        }
        let result = self.conn.execute(
            "UPDATE projects SET name = ?1 WHERE id = ?2",
            params![name, id.0],
        );
        match result {
            Ok(_) => Ok(()),
            Err(err) if is_unique_violation(&err) => {
                Err(StoreError::DuplicateProject(name.to_string()))
            }
            Err(err) => Err(err.into()),
        }
    }

    pub fn set_project_color(&self, id: ProjectId, color: u8) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET color = ?1 WHERE id = ?2",
            params![i64::from(color), id.0],
        )?;
        Ok(())
    }

    /// Archive or restore a project. History is always kept.
    pub fn set_project_archived(&self, id: ProjectId, archived: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE projects SET archived = ?1 WHERE id = ?2",
            params![i64::from(archived), id.0],
        )?;
        Ok(())
    }

    /// Delete a project and every entry recorded against it.
    pub fn delete_project(&self, id: ProjectId) -> Result<()> {
        self.conn
            .execute("DELETE FROM projects WHERE id = ?1", params![id.0])?;
        Ok(())
    }

    /// Projects touched most recently, newest first, for the menu bar's shortlist.
    pub fn recent_project_ids(&self, limit: usize) -> Result<Vec<ProjectId>> {
        let mut stmt = self.conn.prepare(
            "SELECT e.project_id
             FROM time_entries e
             JOIN projects p ON p.id = e.project_id
             WHERE p.archived = 0
             GROUP BY e.project_id
             ORDER BY MAX(e.started_at) DESC
             LIMIT ?1",
        )?;
        let rows = stmt.query_map(params![limit as i64], |row| Ok(ProjectId(row.get(0)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    // -- entries ----------------------------------------------------------

    /// Start a new entry. Fails if one is already open, which is what keeps a
    /// duplicate start (two clicks, or a race with a wake event) from
    /// double-counting.
    pub fn open_entry(&self, project_id: ProjectId, started_at: DateTime<Utc>) -> Result<EntryId> {
        let result = self.conn.execute(
            "INSERT INTO time_entries (project_id, started_at) VALUES (?1, ?2)",
            params![project_id.0, started_at.timestamp()],
        );
        match result {
            Ok(_) => Ok(EntryId(self.conn.last_insert_rowid())),
            Err(err) if is_unique_violation(&err) => Err(StoreError::AlreadyRunning),
            Err(err) => Err(err.into()),
        }
    }

    /// Close whichever entry is open. Returns `None` if none was.
    ///
    /// An end time before the start would be a lie about the past, so it is
    /// clamped to the start, producing a zero-length entry instead.
    pub fn close_open_entry(
        &self,
        ended_at: DateTime<Utc>,
        reason: StopReason,
    ) -> Result<Option<EntryId>> {
        let open: Option<(i64, i64)> = self
            .conn
            .query_row(
                "SELECT id, started_at FROM time_entries WHERE ended_at IS NULL",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;

        let Some((id, started_at)) = open else {
            return Ok(None);
        };

        let end = ended_at.timestamp().max(started_at);
        self.conn.execute(
            "UPDATE time_entries SET ended_at = ?1, stop_reason = ?2 WHERE id = ?3",
            params![end, reason.as_str(), id],
        )?;
        Ok(Some(EntryId(id)))
    }

    /// Record an entry that is already finished, which is how hours typed in
    /// by hand arrive.
    ///
    /// Unlike [`Store::open_entry`] this can never collide with a running
    /// timer, because the row it writes is closed from the moment it exists.
    pub fn insert_entry(
        &self,
        project_id: ProjectId,
        started_at: DateTime<Utc>,
        ended_at: DateTime<Utc>,
        reason: StopReason,
    ) -> Result<EntryId> {
        if ended_at < started_at {
            return Err(StoreError::BackwardsEntry);
        }
        self.conn.execute(
            "INSERT INTO time_entries (project_id, started_at, ended_at, stop_reason)
             VALUES (?1, ?2, ?3, ?4)",
            params![
                project_id.0,
                started_at.timestamp(),
                ended_at.timestamp(),
                reason.as_str()
            ],
        )?;
        Ok(EntryId(self.conn.last_insert_rowid()))
    }

    /// The session the timer should pick up on launch, if the app was killed
    /// while running.
    pub fn running_session(&self) -> Result<Option<Session>> {
        let found = self
            .conn
            .query_row(
                "SELECT project_id, started_at FROM time_entries WHERE ended_at IS NULL",
                [],
                |row| {
                    Ok(Session {
                        project_id: ProjectId(row.get(0)?),
                        started_at: from_unix(row.get(1)?),
                    })
                },
            )
            .optional()?;
        Ok(found)
    }

    /// Every entry overlapping `range`, oldest first. An entry that starts
    /// before the range but runs into it is included, so a session across
    /// midnight still shows up on the later day.
    pub fn entries_in_range(&self, range: DateRange) -> Result<Vec<TimeEntry>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, project_id, started_at, ended_at, stop_reason
             FROM time_entries
             WHERE started_at < ?2 AND (ended_at IS NULL OR ended_at > ?1)
             ORDER BY started_at ASC",
        )?;
        let rows = stmt.query_map(
            params![range.start.timestamp(), range.end.timestamp()],
            row_to_entry,
        )?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// Change an entry's start and end, used when correcting a forgotten stop.
    pub fn update_entry_times(
        &self,
        id: EntryId,
        started_at: DateTime<Utc>,
        ended_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        self.conn.execute(
            "UPDATE time_entries SET started_at = ?1, ended_at = ?2 WHERE id = ?3",
            params![
                started_at.timestamp(),
                ended_at.map(|e| e.timestamp()),
                id.0
            ],
        )?;
        Ok(())
    }

    pub fn delete_entry(&self, id: EntryId) -> Result<()> {
        self.conn
            .execute("DELETE FROM time_entries WHERE id = ?1", params![id.0])?;
        Ok(())
    }

    // -- settings ---------------------------------------------------------

    /// Stored settings, falling back to defaults for anything unset.
    pub fn settings(&self) -> Result<Settings> {
        let defaults = Settings::default();
        Ok(Settings {
            idle_minutes: self
                .read_setting("idle_minutes")?
                .unwrap_or(defaults.idle_minutes),
            auto_resume_under_secs: self
                .read_setting("auto_resume_under_secs")?
                .unwrap_or(defaults.auto_resume_under_secs),
            theme: self
                .read_setting_text("theme")?
                .as_deref()
                .map(ThemeChoice::from_str_lossy)
                .unwrap_or(defaults.theme),
        })
    }

    pub fn save_settings(&self, settings: Settings) -> Result<()> {
        self.write_setting_text("idle_minutes", &settings.idle_minutes.to_string())?;
        self.write_setting_text(
            "auto_resume_under_secs",
            &settings.auto_resume_under_secs.to_string(),
        )?;
        self.write_setting_text("theme", settings.theme.as_str())?;
        Ok(())
    }

    fn read_setting(&self, key: &str) -> Result<Option<u32>> {
        Ok(self
            .read_setting_text(key)?
            .and_then(|value| value.parse().ok()))
    }

    fn read_setting_text(&self, key: &str) -> Result<Option<String>> {
        let raw: Option<String> = self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(raw)
    }

    fn write_setting_text(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }
}

fn row_to_project(row: &rusqlite::Row<'_>) -> rusqlite::Result<Project> {
    Ok(Project {
        id: ProjectId(row.get(0)?),
        name: row.get(1)?,
        color: row.get::<_, i64>(2)? as u8,
        archived: row.get::<_, i64>(3)? != 0,
        created_at: from_unix(row.get(4)?),
    })
}

fn row_to_entry(row: &rusqlite::Row<'_>) -> rusqlite::Result<TimeEntry> {
    let ended_at: Option<i64> = row.get(3)?;
    let reason: Option<String> = row.get(4)?;
    Ok(TimeEntry {
        id: EntryId(row.get(0)?),
        project_id: ProjectId(row.get(1)?),
        started_at: from_unix(row.get(2)?),
        ended_at: ended_at.map(from_unix),
        stop_reason: reason.as_deref().map(StopReason::from_str_lossy),
    })
}

fn from_unix(seconds: i64) -> DateTime<Utc> {
    Utc.timestamp_opt(seconds, 0).single().unwrap_or_default()
}

fn is_unique_violation(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(
            rusqlite::ffi::Error {
                code: rusqlite::ErrorCode::ConstraintViolation,
                ..
            },
            _
        )
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;
    use hourglass_core::report::{DateRange, local_day};

    fn store() -> Store {
        Store::in_memory().expect("in-memory database opens")
    }

    #[test]
    fn a_created_project_comes_back_from_the_list() {
        let store = store();
        let created = store.create_project("Atlas", 3).unwrap();
        let listed = store.projects().unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, created.id);
        assert_eq!(listed[0].name, "Atlas");
        assert_eq!(listed[0].color, 3);
        assert!(!listed[0].archived);
    }

    #[test]
    fn duplicate_names_are_rejected_regardless_of_case() {
        let store = store();
        store.create_project("Atlas", 0).unwrap();
        let err = store.create_project("atlas", 1).unwrap_err();
        assert!(matches!(err, StoreError::DuplicateProject(_)));
    }

    #[test]
    fn a_blank_project_name_is_rejected() {
        let store = store();
        assert!(matches!(
            store.create_project("   ", 0).unwrap_err(),
            StoreError::EmptyProjectName
        ));
    }

    #[test]
    fn archived_projects_sort_after_active_ones() {
        let store = store();
        let zulu = store.create_project("Zulu", 0).unwrap();
        store.create_project("Atlas", 0).unwrap();
        store.set_project_archived(zulu.id, true).unwrap();

        let names: Vec<String> = store
            .projects()
            .unwrap()
            .into_iter()
            .map(|p| p.name)
            .collect();
        assert_eq!(names, vec!["Atlas", "Zulu"]);
    }

    #[test]
    fn only_one_entry_can_be_open_at_a_time() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let beacon = store.create_project("Beacon", 0).unwrap();
        let now = Utc::now();

        store.open_entry(atlas.id, now).unwrap();
        let err = store.open_entry(beacon.id, now).unwrap_err();
        assert!(matches!(err, StoreError::AlreadyRunning));
    }

    #[test]
    fn closing_frees_the_slot_for_the_next_entry() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();

        let first = store.open_entry(atlas.id, now).unwrap();
        let closed = store
            .close_open_entry(now + Duration::minutes(30), StopReason::Manual)
            .unwrap();
        assert_eq!(closed, Some(first));
        assert!(store.open_entry(atlas.id, now + Duration::minutes(31)).is_ok());
    }

    #[test]
    fn closing_when_nothing_runs_reports_nothing() {
        let store = store();
        assert_eq!(
            store
                .close_open_entry(Utc::now(), StopReason::Manual)
                .unwrap(),
            None
        );
    }

    #[test]
    fn an_end_time_before_the_start_is_clamped_to_the_start() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        store.open_entry(atlas.id, now).unwrap();
        store
            .close_open_entry(now - Duration::hours(1), StopReason::Idle)
            .unwrap();

        let entries = store.entries_in_range(local_day(now)).unwrap();
        assert_eq!(entries[0].seconds_at(now), 0);
    }

    #[test]
    fn a_session_left_open_is_recovered_on_the_next_launch() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let started = Utc::now() - Duration::minutes(12);
        store.open_entry(atlas.id, started).unwrap();

        let session = store.running_session().unwrap().unwrap();
        assert_eq!(session.project_id, atlas.id);
        assert_eq!(session.started_at.timestamp(), started.timestamp());
    }

    #[test]
    fn a_range_query_includes_an_entry_that_started_before_it() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        let today = local_day(now);

        // Began ninety minutes before midnight, ended an hour after it.
        store
            .open_entry(atlas.id, today.start - Duration::minutes(90))
            .unwrap();
        store
            .close_open_entry(today.start + Duration::hours(1), StopReason::Manual)
            .unwrap();

        assert_eq!(store.entries_in_range(today).unwrap().len(), 1);
    }

    #[test]
    fn a_range_query_leaves_out_entries_that_finished_earlier() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        let today = local_day(now);

        store
            .open_entry(atlas.id, today.start - Duration::hours(5))
            .unwrap();
        store
            .close_open_entry(today.start - Duration::hours(4), StopReason::Manual)
            .unwrap();

        assert!(store.entries_in_range(today).unwrap().is_empty());
    }

    #[test]
    fn the_stop_reason_survives_a_round_trip() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        store.open_entry(atlas.id, now).unwrap();
        store
            .close_open_entry(now + Duration::minutes(5), StopReason::Sleep)
            .unwrap();

        let entries = store.entries_in_range(local_day(now)).unwrap();
        assert_eq!(entries[0].stop_reason, Some(StopReason::Sleep));
    }

    #[test]
    fn recent_projects_list_the_most_recently_worked_first() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let beacon = store.create_project("Beacon", 0).unwrap();
        let now = Utc::now();

        store.open_entry(atlas.id, now - Duration::hours(3)).unwrap();
        store
            .close_open_entry(now - Duration::hours(2), StopReason::Manual)
            .unwrap();
        store.open_entry(beacon.id, now - Duration::hours(1)).unwrap();
        store.close_open_entry(now, StopReason::Manual).unwrap();

        assert_eq!(
            store.recent_project_ids(5).unwrap(),
            vec![beacon.id, atlas.id]
        );
    }

    #[test]
    fn archived_projects_stay_out_of_the_recent_list() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        store.open_entry(atlas.id, now - Duration::hours(1)).unwrap();
        store.close_open_entry(now, StopReason::Manual).unwrap();
        store.set_project_archived(atlas.id, true).unwrap();

        assert!(store.recent_project_ids(5).unwrap().is_empty());
    }

    #[test]
    fn deleting_a_project_takes_its_entries_with_it() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        store.open_entry(atlas.id, now).unwrap();
        store
            .close_open_entry(now + Duration::minutes(5), StopReason::Manual)
            .unwrap();

        store.delete_project(atlas.id).unwrap();
        assert!(store.entries_in_range(local_day(now)).unwrap().is_empty());
    }

    #[test]
    fn settings_default_until_they_are_saved() {
        let store = store();
        assert_eq!(store.settings().unwrap(), Settings::default());

        let changed = Settings {
            idle_minutes: 4,
            auto_resume_under_secs: 30,
            theme: ThemeChoice::Dark,
        };
        store.save_settings(changed).unwrap();
        assert_eq!(store.settings().unwrap(), changed);
    }

    #[test]
    fn a_hand_written_entry_can_be_added_while_a_timer_is_running() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();

        // The running entry occupies the one open slot.
        store.open_entry(atlas.id, now).unwrap();

        // Yesterday's forgotten afternoon still goes in, because it is closed.
        let started = now - Duration::hours(26);
        let added = store
            .insert_entry(
                atlas.id,
                started,
                started + Duration::hours(2),
                StopReason::Manual,
            )
            .unwrap();

        let entries = store
            .entries_in_range(DateRange {
                start: started - Duration::minutes(1),
                end: started + Duration::hours(3),
            })
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].id, added);
        assert_eq!(entries[0].seconds_at(now), 2 * 3600);
        assert_eq!(entries[0].stop_reason, Some(StopReason::Manual));
    }

    #[test]
    fn a_hand_written_entry_that_ends_before_it_starts_is_refused() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();

        let err = store
            .insert_entry(atlas.id, now, now - Duration::hours(1), StopReason::Manual)
            .unwrap_err();
        assert!(matches!(err, StoreError::BackwardsEntry));
    }

    #[test]
    fn a_range_query_finds_the_entries_a_new_span_would_collide_with() {
        let store = store();
        let atlas = store.create_project("Atlas", 0).unwrap();
        let now = Utc::now();
        let base = now - Duration::hours(5);

        store
            .insert_entry(
                atlas.id,
                base,
                base + Duration::hours(1),
                StopReason::Manual,
            )
            .unwrap();

        // Overlapping the recorded hour finds it.
        let hit = store
            .entries_in_range(DateRange {
                start: base + Duration::minutes(30),
                end: base + Duration::minutes(90),
            })
            .unwrap();
        assert_eq!(hit.len(), 1);

        // Butting up against its end does not.
        let clear = store
            .entries_in_range(DateRange {
                start: base + Duration::hours(1),
                end: base + Duration::hours(2),
            })
            .unwrap();
        assert!(clear.is_empty());
    }
}
