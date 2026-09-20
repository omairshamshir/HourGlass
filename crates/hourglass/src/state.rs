//! The one place that owns the timer, the database, and the platform queue.
//!
//! Views read this and ask it to do things; nothing else writes to the store.
//! Reports are built on demand from cached entries rather than cached
//! themselves, so a running total is never a second stale.

use crate::mac::tray::TrayCommand;
use chrono::{DateTime, Utc};
use hourglass_core::manual::{DraftError, first_conflict, resolve_span};
use hourglass_core::model::{
    EntryId, Project, ProjectId, Settings, StopReason, ThemeChoice, TimeEntry, format_duration,
};
use hourglass_core::report::{
    DateRange, RangeKind, Report, build_report, csv_filename, range_for, report_to_csv,
};
use hourglass_core::timer::{Effect, SystemEvent, Timer};
use hourglass_store::{Store, StoreError};
use std::path::PathBuf;

/// A short message shown under the header: a confirmation or a failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Status {
    pub message: String,
    pub is_error: bool,
    pub shown_at: DateTime<Utc>,
}

/// What a tick changed, so the caller knows whether to redraw or reopen.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct TickOutcome {
    /// Something visible changed and the window should repaint.
    pub redraw: bool,
    /// The user asked for the window, or needs to answer a resume prompt.
    pub surface_window: bool,
    /// The user asked to quit.
    pub quit: bool,
}

/// Everything the app knows.
pub struct AppState {
    store: Store,
    timer: Timer,
    projects: Vec<Project>,
    range: RangeKind,
    entries_today: Vec<TimeEntry>,
    entries_range: Vec<TimeEntry>,
    status: Option<Status>,
    /// The clock second the UI last drew, so ticks that change nothing are free.
    last_drawn_second: i64,
}

impl AppState {
    /// Open the database and pick up any session left running by a crash.
    pub fn load(store: Store) -> Self {
        let settings = store.settings().unwrap_or_default();
        let session = store.running_session().unwrap_or(None);

        let mut state = AppState {
            store,
            timer: Timer::resumed_from(session, settings),
            projects: Vec::new(),
            range: RangeKind::Today,
            entries_today: Vec::new(),
            entries_range: Vec::new(),
            status: None,
            last_drawn_second: 0,
        };
        state.reload();
        state
    }

    // -- reading ----------------------------------------------------------

    pub fn projects(&self) -> &[Project] {
        &self.projects
    }

    /// Projects offered for starting: the archived ones are history only.
    pub fn active_projects(&self) -> impl Iterator<Item = &Project> {
        self.projects.iter().filter(|p| !p.archived)
    }

    pub fn project(&self, id: ProjectId) -> Option<&Project> {
        self.projects.iter().find(|p| p.id == id)
    }

    /// The project the clock is running against.
    pub fn running_project(&self) -> Option<&Project> {
        self.timer.session().and_then(|s| self.project(s.project_id))
    }

    /// The project waiting on a resume answer, and how long the user was away.
    ///
    /// `now` is passed in rather than read here so the prompt's "away 42
    /// minutes" agrees with the clock drawn beside it in the same frame.
    pub fn pending_resume(&self, now: DateTime<Utc>) -> Option<(&Project, i64)> {
        if !self.timer.is_prompting() {
            return None;
        }
        let paused = self.timer.paused()?;
        let away = (now - paused.paused_at).num_seconds().max(0);
        Some((self.project(paused.project_id)?, away))
    }

    pub fn is_running(&self) -> bool {
        self.timer.is_running()
    }

    pub fn elapsed_seconds(&self, now: DateTime<Utc>) -> i64 {
        self.timer.elapsed_seconds(now)
    }

    pub fn settings(&self) -> Settings {
        self.timer.settings()
    }

    pub fn status(&self) -> Option<&Status> {
        self.status.as_ref()
    }

    /// Today's numbers, always fresh against `now`.
    pub fn today(&self, now: DateTime<Utc>) -> Report {
        build_report(
            RangeKind::Today,
            range_for(RangeKind::Today, now),
            &self.projects,
            &self.entries_today,
            now,
        )
    }

    /// Numbers for the range the user has selected.
    pub fn report(&self, now: DateTime<Utc>) -> Report {
        build_report(
            self.range,
            range_for(self.range, now),
            &self.projects,
            &self.entries_range,
            now,
        )
    }

    // -- timer ------------------------------------------------------------

    /// Start a project, switching away from whatever was running.
    pub fn start(&mut self, project_id: ProjectId, now: DateTime<Utc>) {
        let effects = self.timer.start(project_id, now);
        self.apply(effects);
    }

    pub fn stop(&mut self, now: DateTime<Utc>) {
        let effects = self.timer.stop(now);
        self.apply(effects);
    }

    /// The menu bar's one-key action: stop if running, otherwise start the
    /// project the user worked on last.
    pub fn toggle(&mut self, now: DateTime<Utc>) {
        if self.is_running() {
            self.stop(now);
            return;
        }
        if let Some(project) = self.last_used_project() {
            self.start(project, now);
        }
    }

    pub fn resume(&mut self, now: DateTime<Utc>) {
        let effects = self.timer.resume(now);
        self.apply(effects);
    }

    pub fn dismiss_prompt(&mut self) {
        let effects = self.timer.discard_pause();
        self.apply(effects);
    }

    // -- projects ---------------------------------------------------------

    /// Add a project, giving it the next unused swatch.
    pub fn create_project(&mut self, name: &str) -> Option<ProjectId> {
        let color = (self.projects.len() % 8) as u8;
        match self.store.create_project(name, color) {
            Ok(project) => {
                self.note(format!("Added {}", project.name), false);
                self.reload();
                Some(project.id)
            }
            Err(err) => {
                self.fail(err);
                None
            }
        }
    }

    pub fn rename_project(&mut self, id: ProjectId, name: &str) {
        if let Err(err) = self.store.rename_project(id, name) {
            self.fail(err);
            return;
        }
        self.reload();
    }

    /// Archive or restore. Archiving the running project stops the clock
    /// first, because a hidden project cannot be stopped from the menu.
    pub fn set_project_archived(&mut self, id: ProjectId, archived: bool, now: DateTime<Utc>) {
        if archived && self.timer.session().map(|s| s.project_id) == Some(id) {
            self.stop(now);
        }
        if let Err(err) = self.store.set_project_archived(id, archived) {
            self.fail(err);
            return;
        }
        self.reload();
    }

    /// Delete a project and its history.
    pub fn delete_project(&mut self, id: ProjectId, now: DateTime<Utc>) {
        if self.timer.session().map(|s| s.project_id) == Some(id) {
            self.stop(now);
        }
        let name = self.project(id).map(|p| p.name.clone());
        if let Err(err) = self.store.delete_project(id) {
            self.fail(err);
            return;
        }
        if let Some(name) = name {
            self.note(format!("Deleted {name} and its entries"), false);
        }
        self.reload();
    }

    // -- entries ----------------------------------------------------------

    pub fn delete_entry(&mut self, id: EntryId) {
        if let Err(err) = self.store.delete_entry(id) {
            self.fail(err);
            return;
        }
        self.reload();
    }

    /// Record hours the user typed in rather than timed.
    ///
    /// Returns whether the entry was written. On failure the reason is left in
    /// the status line and the caller keeps the form open so it can be fixed,
    /// because a rejected entry usually has one wrong field, not four.
    pub fn add_manual_entry(
        &mut self,
        project_name: &str,
        date: &str,
        start: &str,
        end: &str,
        now: DateTime<Utc>,
    ) -> bool {
        match self.write_manual_entry(project_name, date, start, end, now) {
            Ok(summary) => {
                self.note(summary, false);
                self.reload();
                true
            }
            Err(reason) => {
                self.note(reason, true);
                false
            }
        }
    }

    /// The checking half of [`AppState::add_manual_entry`], split out so every
    /// refusal is a single `?` rather than a nest of early returns.
    fn write_manual_entry(
        &mut self,
        project_name: &str,
        date: &str,
        start: &str,
        end: &str,
        now: DateTime<Utc>,
    ) -> Result<String, String> {
        let typed = project_name.trim();
        if typed.is_empty() {
            return Err(DraftError::NoProject.to_string());
        }

        let wanted = typed.to_lowercase();
        let project = self
            .projects
            .iter()
            .find(|project| project.name.to_lowercase() == wanted)
            .ok_or_else(|| DraftError::UnknownProject(typed.to_string()).to_string())?;
        let (project_id, matched_name) = (project.id, project.name.clone());

        let (started_at, ended_at) =
            resolve_span(date, start, end, now).map_err(|err| err.to_string())?;

        // Hours are only honest if they are claimed once, so refuse to write
        // over a span some other entry already covers.
        let existing = self
            .store
            .entries_in_range(DateRange {
                start: started_at,
                end: ended_at,
            })
            .map_err(|err| err.to_string())?;
        if let Some(clash) = first_conflict(&existing, started_at, ended_at, now) {
            let owner = self
                .project(clash.project_id)
                .map(|project| project.name.clone())
                .unwrap_or_else(|| "another project".to_string());
            return Err(DraftError::Overlaps(owner).to_string());
        }

        self.store
            .insert_entry(project_id, started_at, ended_at, StopReason::Manual)
            .map_err(|err| err.to_string())?;

        let logged = format_duration((ended_at - started_at).num_seconds());
        Ok(format!("Added {logged} to {matched_name}"))
    }

    // -- settings ---------------------------------------------------------

    /// Change which palette the app uses, and remember it across launches.
    pub fn set_theme(&mut self, choice: ThemeChoice) {
        let mut settings = self.settings();
        if settings.theme == choice {
            return;
        }
        settings.theme = choice;
        self.timer.set_settings(settings);
        if let Err(err) = self.store.save_settings(settings) {
            self.fail(err);
        }
    }

    // -- reports ----------------------------------------------------------

    pub fn set_range(&mut self, range: RangeKind) {
        if self.range == range {
            return;
        }
        self.range = range;
        self.reload();
    }

    /// Write the current report to `~/Downloads` and say where it went.
    pub fn export_csv(&mut self, now: DateTime<Utc>) -> Option<PathBuf> {
        let report = self.report(now);
        if report.entries.is_empty() {
            self.note("Nothing to export in this range".to_string(), true);
            return None;
        }

        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        let downloads = home.join("Downloads");
        let path = downloads.join(csv_filename(&report));

        match std::fs::write(&path, report_to_csv(&report)) {
            Ok(()) => {
                self.note(format!("Exported to Downloads/{}", csv_filename(&report)), false);
                Some(path)
            }
            Err(err) => {
                self.note(format!("Could not write the file: {err}"), true);
                None
            }
        }
    }

    // -- the tick ---------------------------------------------------------

    /// Fold in what happened since the last tick and report what changed.
    ///
    /// The caller gathers system events and menu clicks; this decides what
    /// they mean. Called several times a second, so a tick with nothing in it
    /// does no work at all.
    pub fn tick(
        &mut self,
        now: DateTime<Utc>,
        events: &[SystemEvent],
        tray_commands: &[TrayCommand],
    ) -> TickOutcome {
        let mut outcome = TickOutcome::default();
        let was_prompting = self.timer.is_prompting();

        for event in events {
            let effects = self.timer.handle(*event);
            if !effects.is_empty() {
                outcome.redraw = true;
            }
            self.apply(effects);
        }

        for command in tray_commands {
            outcome.redraw = true;
            match *command {
                TrayCommand::Start(id) => self.start(id, now),
                TrayCommand::Stop => self.stop(now),
                TrayCommand::Resume => self.resume(now),
                TrayCommand::Dismiss => self.dismiss_prompt(),
                TrayCommand::OpenWindow => outcome.surface_window = true,
                TrayCommand::Quit => outcome.quit = true,
            }
        }

        // A prompt that just appeared needs the window in front to be answered.
        if self.timer.is_prompting() && !was_prompting {
            outcome.surface_window = true;
        }

        // While running, redraw once a second and no more often.
        if self.timer.is_running() {
            let second = self.elapsed_seconds(now);
            if second != self.last_drawn_second {
                self.last_drawn_second = second;
                outcome.redraw = true;
            }
        }

        // Let a status message fade out on its own after a few seconds.
        if let Some(status) = &self.status
            && (now - status.shown_at).num_seconds() >= 5
        {
            self.status = None;
            outcome.redraw = true;
        }

        outcome
    }

    // -- internals --------------------------------------------------------

    /// Carry out what the timer decided. Prompt effects need no work here:
    /// the views read the timer's own prompting state.
    fn apply(&mut self, effects: Vec<Effect>) {
        if effects.is_empty() {
            return;
        }

        for effect in effects {
            let result = match effect {
                Effect::CloseEntry { ended_at, reason } => {
                    self.store.close_open_entry(ended_at, reason).map(|_| ())
                }
                Effect::OpenEntry {
                    project_id,
                    started_at,
                } => self.store.open_entry(project_id, started_at).map(|_| ()),
                Effect::AskToResume { .. } | Effect::CancelPrompt => Ok(()),
            };

            if let Err(err) = result {
                self.fail(err);
            }
        }
        self.reload();
    }

    /// Re-read everything the views display.
    fn reload(&mut self) {
        let now = Utc::now();
        match self.store.projects() {
            Ok(projects) => self.projects = projects,
            Err(err) => self.fail(err),
        }
        match self.store.entries_in_range(range_for(RangeKind::Today, now)) {
            Ok(entries) => self.entries_today = entries,
            Err(err) => self.fail(err),
        }
        self.entries_range = if self.range == RangeKind::Today {
            self.entries_today.clone()
        } else {
            match self.store.entries_in_range(range_for(self.range, now)) {
                Ok(entries) => entries,
                Err(err) => {
                    self.fail(err);
                    Vec::new()
                }
            }
        };
    }

    /// The project to start when the user just wants the clock running again.
    fn last_used_project(&self) -> Option<ProjectId> {
        self.store
            .recent_project_ids(1)
            .ok()
            .and_then(|ids| ids.first().copied())
            .or_else(|| self.active_projects().next().map(|p| p.id))
    }

    fn note(&mut self, message: String, is_error: bool) {
        self.status = Some(Status {
            message,
            is_error,
            shown_at: Utc::now(),
        });
    }

    fn fail(&mut self, err: StoreError) {
        self.note(err.to_string(), true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Duration, Local, TimeZone};
    use hourglass_core::model::format_duration;

    fn state() -> AppState {
        AppState::load(Store::in_memory().unwrap())
    }

    #[test]
    fn a_new_database_has_no_projects_and_a_stopped_clock() {
        let state = state();
        assert!(state.projects().is_empty());
        assert!(!state.is_running());
        assert_eq!(state.today(Utc::now()).total_seconds, 0);
    }

    #[test]
    fn starting_a_project_puts_time_on_todays_report() {
        let mut state = state();
        let id = state.create_project("Atlas").unwrap();
        let start = Utc::now() - Duration::minutes(30);

        state.start(id, start);
        assert!(state.is_running());
        assert_eq!(state.running_project().unwrap().name, "Atlas");

        let today = state.today(Utc::now());
        assert_eq!(format_duration(today.total_seconds), "30m");
    }

    #[test]
    fn switching_projects_splits_the_time_between_them() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let beacon = state.create_project("Beacon").unwrap();
        let now = Utc::now();

        state.start(atlas, now - Duration::minutes(60));
        state.start(beacon, now - Duration::minutes(20));
        state.stop(now);

        let today = state.today(now);
        assert_eq!(today.totals.len(), 2);
        assert_eq!(today.total_seconds, 60 * 60);
        let atlas_total = today
            .totals
            .iter()
            .find(|t| t.project_id == atlas)
            .unwrap()
            .seconds;
        assert_eq!(atlas_total, 40 * 60);
    }

    #[test]
    fn a_duplicate_project_name_reports_an_error_instead_of_adding_one() {
        let mut state = state();
        state.create_project("Atlas").unwrap();
        assert!(state.create_project("Atlas").is_none());
        assert!(state.status().unwrap().is_error);
        assert_eq!(state.projects().len(), 1);
    }

    #[test]
    fn archiving_the_running_project_stops_the_clock() {
        let mut state = state();
        let id = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(id, now - Duration::minutes(5));

        state.set_project_archived(id, true, now);
        assert!(!state.is_running());
        assert_eq!(state.active_projects().count(), 0);
        // The work already done is still on the books.
        assert_eq!(state.today(now).total_seconds, 5 * 60);
    }

    #[test]
    fn toggling_with_nothing_running_restarts_the_last_project() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(atlas, now - Duration::hours(2));
        state.stop(now - Duration::hours(1));

        state.toggle(now);
        assert!(state.is_running());
        assert_eq!(state.running_project().unwrap().id, atlas);
    }

    #[test]
    fn toggling_while_running_stops_the_clock() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(atlas, now - Duration::minutes(5));

        state.toggle(now);
        assert!(!state.is_running());
    }

    #[test]
    fn toggling_with_no_projects_at_all_does_nothing() {
        let mut state = state();
        state.toggle(Utc::now());
        assert!(!state.is_running());
    }

    #[test]
    fn a_sleep_signal_pauses_and_a_wake_offers_to_resume() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(atlas, now - Duration::hours(1));

        state.tick(now, &[SystemEvent::WillSleep { at: now }], &[]);
        assert!(!state.is_running());

        let woke = now + Duration::hours(3);
        let outcome = state.tick(woke, &[SystemEvent::DidWake { at: woke }], &[]);
        assert!(outcome.surface_window, "a long pause must ask the user");
        let (project, away) = state.pending_resume(woke).unwrap();
        assert_eq!(project.id, atlas);
        assert!(away >= 3 * 3600);
    }

    #[test]
    fn a_short_sleep_resumes_without_asking_or_surfacing_the_window() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(atlas, now - Duration::hours(1));

        state.tick(now, &[SystemEvent::WillSleep { at: now }], &[]);
        let woke = now + Duration::seconds(45);
        let outcome = state.tick(woke, &[SystemEvent::DidWake { at: woke }], &[]);

        assert!(state.is_running());
        assert!(!outcome.surface_window);
        assert!(state.pending_resume(Utc::now()).is_none());
    }

    #[test]
    fn time_asleep_is_never_billed() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();

        // An hour of work, three hours asleep, then resume.
        state.start(atlas, now - Duration::hours(4));
        let slept = now - Duration::hours(3);
        state.tick(slept, &[SystemEvent::WillSleep { at: slept }], &[]);
        state.tick(now, &[SystemEvent::DidWake { at: now }], &[]);
        state.resume(now);

        assert_eq!(state.today(now).total_seconds, 3_600);
    }

    #[test]
    fn answering_the_prompt_starts_a_new_entry_and_clears_it() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(atlas, now - Duration::hours(2));

        let slept = now - Duration::hours(1);
        state.tick(slept, &[SystemEvent::WillSleep { at: slept }], &[]);
        state.tick(now, &[SystemEvent::DidWake { at: now }], &[]);

        state.resume(now);
        assert!(state.is_running());
        assert!(state.pending_resume(Utc::now()).is_none());
    }

    #[test]
    fn idle_time_is_taken_off_the_clock() {
        let mut state = state();
        let atlas = state.create_project("Atlas").unwrap();
        let now = Utc::now();
        state.start(atlas, now - Duration::minutes(60));

        // Stopped typing forty minutes ago; noticed ten minutes later.
        let quiet_since = now - Duration::minutes(40);
        state.tick(now, &[SystemEvent::WentIdle { since: quiet_since }], &[]);

        assert!(!state.is_running());
        assert_eq!(state.today(now).total_seconds, 20 * 60);
    }

    #[test]
    fn a_menu_command_to_quit_is_reported_to_the_caller() {
        let mut state = state();
        let outcome = state.tick(Utc::now(), &[], &[TrayCommand::Quit]);
        assert!(outcome.quit);
    }

    #[test]
    fn exporting_an_empty_range_explains_itself_rather_than_writing_a_file() {
        let mut state = state();
        assert!(state.export_csv(Utc::now()).is_none());
        assert!(state.status().unwrap().is_error);
    }

    #[test]
    fn a_status_message_clears_itself_after_a_few_seconds() {
        let mut state = state();
        state.create_project("Atlas").unwrap();
        assert!(state.status().is_some());

        state.tick(Utc::now() + Duration::seconds(6), &[], &[]);
        assert!(state.status().is_none());
    }

    // -- hours typed in by hand -------------------------------------------

    /// A state with one project and a fixed "now" late enough in the local day
    /// that morning hours are safely in the past.
    fn state_at_six_pm() -> (AppState, DateTime<Utc>) {
        let mut state = state();
        state.create_project("Atlas").unwrap();

        let today = Utc::now().with_timezone(&Local).date_naive();
        let six_pm = Local
            .from_local_datetime(&today.and_hms_opt(18, 0, 0).unwrap())
            .earliest()
            .unwrap()
            .with_timezone(&Utc);

        (state, six_pm)
    }

    #[test]
    fn hours_typed_in_by_hand_land_on_the_day_they_name() {
        let (mut state, now) = state_at_six_pm();

        assert!(state.add_manual_entry("Atlas", "today", "9:30", "11:00", now));
        assert_eq!(format_duration(state.today(now).total_seconds), "1h 30m");
        assert!(!state.status().unwrap().is_error);
    }

    #[test]
    fn a_hand_written_entry_is_marked_as_one() {
        let (mut state, now) = state_at_six_pm();
        state.add_manual_entry("Atlas", "today", "9:00", "10:00", now);

        let entries = state.today(now).entries;
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].stop_reason, Some(StopReason::Manual));
    }

    #[test]
    fn the_project_name_is_matched_without_regard_to_case() {
        let (mut state, now) = state_at_six_pm();
        assert!(state.add_manual_entry("  atlas ", "today", "9:00", "10:00", now));
    }

    #[test]
    fn an_unknown_project_is_refused_and_says_so() {
        let (mut state, now) = state_at_six_pm();

        assert!(!state.add_manual_entry("Borealis", "today", "9:00", "10:00", now));
        let status = state.status().unwrap();
        assert!(status.is_error);
        assert!(status.message.contains("Borealis"));
        assert_eq!(state.today(now).total_seconds, 0);
    }

    #[test]
    fn hours_cannot_be_written_over_hours_already_recorded() {
        let (mut state, now) = state_at_six_pm();
        assert!(state.add_manual_entry("Atlas", "today", "9:00", "12:00", now));

        // The whole point of the app is that a recorded hour is claimed once.
        assert!(!state.add_manual_entry("Atlas", "today", "11:00", "13:00", now));
        assert!(state.status().unwrap().is_error);
        assert_eq!(state.today(now).total_seconds, 3 * 3600);
    }

    #[test]
    fn hours_butting_up_against_an_existing_entry_are_allowed() {
        let (mut state, now) = state_at_six_pm();
        assert!(state.add_manual_entry("Atlas", "today", "9:00", "10:00", now));
        assert!(state.add_manual_entry("Atlas", "today", "10:00", "11:00", now));
        assert_eq!(state.today(now).total_seconds, 2 * 3600);
    }

    #[test]
    fn hand_written_hours_cannot_cover_the_session_running_right_now() {
        let (mut state, now) = state_at_six_pm();
        let id = state.projects()[0].id;
        state.start(id, now - Duration::hours(1));

        assert!(!state.add_manual_entry("Atlas", "today", "17:30", "17:45", now));
        assert!(state.status().unwrap().is_error);
    }

    #[test]
    fn a_hand_written_entry_can_be_added_while_the_clock_runs_elsewhere() {
        let (mut state, now) = state_at_six_pm();
        let id = state.projects()[0].id;
        state.start(id, now - Duration::minutes(30));

        // Morning hours do not touch the running session, so they go in and
        // the clock keeps counting.
        assert!(state.add_manual_entry("Atlas", "today", "9:00", "10:00", now));
        assert!(state.is_running());
    }

    // -- appearance --------------------------------------------------------

    #[test]
    fn the_theme_choice_is_remembered() {
        let mut state = state();
        assert_eq!(state.settings().theme, ThemeChoice::System);

        state.set_theme(ThemeChoice::Dark);
        assert_eq!(state.settings().theme, ThemeChoice::Dark);
    }

    #[test]
    fn the_theme_choice_survives_a_restart() {
        let file = std::env::temp_dir().join(format!("hourglass-theme-{}.db", std::process::id()));
        let _ = std::fs::remove_file(&file);

        let mut state = AppState::load(Store::open(&file).unwrap());
        state.set_theme(ThemeChoice::Light);
        drop(state);

        let reopened = AppState::load(Store::open(&file).unwrap());
        assert_eq!(reopened.settings().theme, ThemeChoice::Light);

        let _ = std::fs::remove_file(&file);
    }
}
