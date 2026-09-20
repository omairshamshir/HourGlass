//! The timer state machine.
//!
//! This module owns every decision about when the clock runs, including the
//! rules for sleep, screen lock, and idle. It touches no database and no
//! AppKit, so all of that behaviour is unit-testable at any timestamp.
//!
//! Entry ids never appear here. The machine knows only that at most one entry
//! is open; the store closes "the open entry" when told to.

use crate::model::{ProjectId, Settings, StopReason};
use chrono::{DateTime, Duration, Utc};

/// The clock is currently counting time against a project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Session {
    pub project_id: ProjectId,
    pub started_at: DateTime<Utc>,
}

/// The app stopped the clock on the user's behalf and remembers where to pick up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Paused {
    pub project_id: ProjectId,
    pub paused_at: DateTime<Utc>,
    pub reason: StopReason,
}

/// Something the machine observed about the Mac, with the moment it happened.
///
/// Timestamps are passed in rather than read from the clock because the honest
/// moment is often in the past: a wake notification arrives long after sleep
/// began, and idle is only noticed minutes after the user stopped typing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemEvent {
    /// macOS is about to sleep.
    WillSleep { at: DateTime<Utc> },
    /// The machine woke up.
    DidWake { at: DateTime<Utc> },
    /// The screen locked or the screen saver started.
    ScreenLocked { at: DateTime<Utc> },
    /// The user came back to an unlocked screen.
    ScreenUnlocked { at: DateTime<Utc> },
    /// No input since `since`, and that gap has passed the idle threshold.
    WentIdle { since: DateTime<Utc> },
    /// Input resumed after an idle stretch.
    BecameActive { at: DateTime<Utc> },
}

/// Work for the caller to carry out: write to the store, or ask the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Effect {
    /// Close the open entry at this instant with this reason.
    CloseEntry {
        ended_at: DateTime<Utc>,
        reason: StopReason,
    },
    /// Insert a new open entry for this project.
    OpenEntry {
        project_id: ProjectId,
        started_at: DateTime<Utc>,
    },
    /// Ask whether to resume; the pause was long enough that silence would lie.
    AskToResume {
        project_id: ProjectId,
        away_seconds: i64,
        reason: StopReason,
    },
    /// Take down a resume prompt that is no longer relevant.
    CancelPrompt,
}

/// Timer state plus the policy that drives it.
#[derive(Debug, Clone)]
pub struct Timer {
    session: Option<Session>,
    paused: Option<Paused>,
    prompting: bool,
    settings: Settings,
}

impl Timer {
    /// A stopped timer with the given policy.
    pub fn new(settings: Settings) -> Self {
        Timer {
            session: None,
            paused: None,
            prompting: false,
            settings,
        }
    }

    /// Rebuild the machine from a session already open in the database, which
    /// is how the app survives a restart or a crash mid-session.
    pub fn resumed_from(session: Option<Session>, settings: Settings) -> Self {
        Timer {
            session,
            paused: None,
            prompting: false,
            settings,
        }
    }

    pub fn settings(&self) -> Settings {
        self.settings
    }

    pub fn set_settings(&mut self, settings: Settings) {
        self.settings = settings;
    }

    /// The running session, if the clock is counting.
    pub fn session(&self) -> Option<Session> {
        self.session
    }

    /// The project waiting to be resumed, if the app paused itself.
    pub fn paused(&self) -> Option<Paused> {
        self.paused
    }

    pub fn is_running(&self) -> bool {
        self.session.is_some()
    }

    /// True while a resume prompt is on screen.
    pub fn is_prompting(&self) -> bool {
        self.prompting
    }

    /// Seconds on the clock for the running session.
    pub fn elapsed_seconds(&self, now: DateTime<Utc>) -> i64 {
        self.session
            .map(|s| (now - s.started_at).num_seconds().max(0))
            .unwrap_or(0)
    }

    /// Start `project`. A different running project is stopped first and the
    /// handover is recorded as a switch, so no second is counted twice.
    /// Starting the project that is already running does nothing.
    pub fn start(&mut self, project_id: ProjectId, now: DateTime<Utc>) -> Vec<Effect> {
        if self.session.map(|s| s.project_id) == Some(project_id) {
            return Vec::new();
        }

        let mut effects = self.clear_prompt();
        if self.session.is_some() {
            effects.push(Effect::CloseEntry {
                ended_at: now,
                reason: StopReason::Switch,
            });
        }
        self.paused = None;
        self.session = Some(Session {
            project_id,
            started_at: now,
        });
        effects.push(Effect::OpenEntry {
            project_id,
            started_at: now,
        });
        effects
    }

    /// Stop the clock at the user's request. Also clears any pending resume,
    /// because an explicit stop is the user saying they are done.
    pub fn stop(&mut self, now: DateTime<Utc>) -> Vec<Effect> {
        let mut effects = self.clear_prompt();
        self.paused = None;
        if self.session.take().is_some() {
            effects.push(Effect::CloseEntry {
                ended_at: now,
                reason: StopReason::Manual,
            });
        }
        effects
    }

    /// Answer a resume prompt with "keep going". Time spent away stays unbilled:
    /// the new entry opens at `now`, not at the moment of the pause.
    pub fn resume(&mut self, now: DateTime<Utc>) -> Vec<Effect> {
        let Some(paused) = self.paused.take() else {
            return self.clear_prompt();
        };
        self.prompting = false;
        self.session = Some(Session {
            project_id: paused.project_id,
            started_at: now,
        });
        vec![Effect::OpenEntry {
            project_id: paused.project_id,
            started_at: now,
        }]
    }

    /// Answer a resume prompt with "leave it stopped".
    pub fn discard_pause(&mut self) -> Vec<Effect> {
        self.paused = None;
        self.clear_prompt()
    }

    /// Feed the machine something that happened to the Mac.
    pub fn handle(&mut self, event: SystemEvent) -> Vec<Effect> {
        match event {
            SystemEvent::WillSleep { at } => self.pause(at, StopReason::Sleep),
            SystemEvent::ScreenLocked { at } => self.pause(at, StopReason::Lock),
            SystemEvent::WentIdle { since } => self.pause(since, StopReason::Idle),
            SystemEvent::DidWake { at }
            | SystemEvent::ScreenUnlocked { at }
            | SystemEvent::BecameActive { at } => self.came_back(at),
        }
    }

    /// Stop the clock without the user asking, remembering where to resume.
    ///
    /// `at` is the honest moment work stopped, which is earlier than "now" for
    /// idle. A pause never overwrites an earlier pause: if the screen locks and
    /// then the Mac sleeps, the first reason is the true one.
    fn pause(&mut self, at: DateTime<Utc>, reason: StopReason) -> Vec<Effect> {
        let Some(session) = self.session.take() else {
            return Vec::new();
        };

        // Never let a backdated pause run past the start of the entry it closes.
        let ended_at = at.max(session.started_at);
        self.paused = Some(Paused {
            project_id: session.project_id,
            paused_at: ended_at,
            reason,
        });
        vec![Effect::CloseEntry { ended_at, reason }]
    }

    /// The user is back. Short absences resume silently; longer ones ask first,
    /// because silently restarting the clock after lunch invents hours.
    fn came_back(&mut self, at: DateTime<Utc>) -> Vec<Effect> {
        let Some(paused) = self.paused else {
            return Vec::new();
        };

        let away = (at - paused.paused_at).num_seconds().max(0);
        if away <= i64::from(self.settings.auto_resume_under_secs) {
            return self.resume(at);
        }

        if self.prompting {
            return Vec::new();
        }
        self.prompting = true;
        vec![Effect::AskToResume {
            project_id: paused.project_id,
            away_seconds: away,
            reason: paused.reason,
        }]
    }

    /// Drop a prompt that is on screen, telling the caller to take it down too.
    fn clear_prompt(&mut self) -> Vec<Effect> {
        if self.prompting {
            self.prompting = false;
            vec![Effect::CancelPrompt]
        } else {
            Vec::new()
        }
    }
}

/// How long the machine must be quiet before [`SystemEvent::WentIdle`] applies.
pub fn idle_threshold(settings: Settings) -> Duration {
    Duration::minutes(i64::from(settings.idle_minutes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn at(minute: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap() + Duration::minutes(minute)
    }

    fn secs(second: i64) -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 9, 7, 9, 0, 0).unwrap() + Duration::seconds(second)
    }

    const ALPHA: ProjectId = ProjectId(1);
    const BETA: ProjectId = ProjectId(2);

    fn timer() -> Timer {
        Timer::new(Settings::default())
    }

    #[test]
    fn starting_opens_one_entry() {
        let mut t = timer();
        let effects = t.start(ALPHA, at(0));
        assert_eq!(
            effects,
            vec![Effect::OpenEntry {
                project_id: ALPHA,
                started_at: at(0)
            }]
        );
        assert!(t.is_running());
    }

    #[test]
    fn starting_the_running_project_again_is_a_no_op() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        assert!(t.start(ALPHA, at(5)).is_empty());
        assert_eq!(t.session().unwrap().started_at, at(0));
    }

    #[test]
    fn switching_closes_the_old_entry_at_the_same_instant_it_opens_the_new_one() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        let effects = t.start(BETA, at(30));
        assert_eq!(
            effects,
            vec![
                Effect::CloseEntry {
                    ended_at: at(30),
                    reason: StopReason::Switch
                },
                Effect::OpenEntry {
                    project_id: BETA,
                    started_at: at(30)
                },
            ]
        );
    }

    #[test]
    fn stopping_closes_the_entry_and_stops_the_clock() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        let effects = t.stop(at(45));
        assert_eq!(
            effects,
            vec![Effect::CloseEntry {
                ended_at: at(45),
                reason: StopReason::Manual
            }]
        );
        assert!(!t.is_running());
        assert_eq!(t.elapsed_seconds(at(60)), 0);
    }

    #[test]
    fn stopping_an_idle_timer_does_nothing() {
        let mut t = timer();
        assert!(t.stop(at(1)).is_empty());
    }

    #[test]
    fn sleep_closes_the_entry_at_the_sleep_moment() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        let effects = t.handle(SystemEvent::WillSleep { at: at(20) });
        assert_eq!(
            effects,
            vec![Effect::CloseEntry {
                ended_at: at(20),
                reason: StopReason::Sleep
            }]
        );
        assert!(!t.is_running());
        assert_eq!(t.paused().unwrap().project_id, ALPHA);
    }

    #[test]
    fn a_short_sleep_resumes_without_asking() {
        let mut t = timer();
        t.start(ALPHA, secs(0));
        t.handle(SystemEvent::WillSleep { at: secs(600) });
        let effects = t.handle(SystemEvent::DidWake { at: secs(690) });
        assert_eq!(
            effects,
            vec![Effect::OpenEntry {
                project_id: ALPHA,
                started_at: secs(690)
            }]
        );
        assert!(t.is_running());
        assert!(t.paused().is_none());
    }

    #[test]
    fn a_long_sleep_asks_before_resuming() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::WillSleep { at: at(10) });
        let effects = t.handle(SystemEvent::DidWake { at: at(70) });
        assert_eq!(
            effects,
            vec![Effect::AskToResume {
                project_id: ALPHA,
                away_seconds: 3_600,
                reason: StopReason::Sleep
            }]
        );
        assert!(!t.is_running());
        assert!(t.is_prompting());
    }

    #[test]
    fn answering_the_prompt_starts_a_fresh_entry_at_the_answer_time() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::WillSleep { at: at(10) });
        t.handle(SystemEvent::DidWake { at: at(70) });
        let effects = t.resume(at(71));
        assert_eq!(
            effects,
            vec![Effect::OpenEntry {
                project_id: ALPHA,
                started_at: at(71)
            }]
        );
        assert!(!t.is_prompting());
        // The hour asleep is not billed: the new entry starts after the wake.
        assert_eq!(t.elapsed_seconds(at(81)), 600);
    }

    #[test]
    fn declining_the_prompt_leaves_the_timer_stopped() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::WillSleep { at: at(10) });
        t.handle(SystemEvent::DidWake { at: at(70) });
        assert_eq!(t.discard_pause(), vec![Effect::CancelPrompt]);
        assert!(!t.is_running());
        assert!(t.paused().is_none());
    }

    #[test]
    fn idle_is_backdated_to_the_last_input_not_to_now() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        let effects = t.handle(SystemEvent::WentIdle { since: at(15) });
        assert_eq!(
            effects,
            vec![Effect::CloseEntry {
                ended_at: at(15),
                reason: StopReason::Idle
            }]
        );
    }

    #[test]
    fn returning_from_idle_always_asks_because_the_gap_exceeds_the_threshold() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::WentIdle { since: at(15) });
        let effects = t.handle(SystemEvent::BecameActive { at: at(25) });
        assert_eq!(
            effects,
            vec![Effect::AskToResume {
                project_id: ALPHA,
                away_seconds: 600,
                reason: StopReason::Idle
            }]
        );
    }

    #[test]
    fn locking_the_screen_pauses_like_sleep() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        assert_eq!(
            t.handle(SystemEvent::ScreenLocked { at: at(5) }),
            vec![Effect::CloseEntry {
                ended_at: at(5),
                reason: StopReason::Lock
            }]
        );
        assert_eq!(
            t.handle(SystemEvent::ScreenUnlocked { at: at(6) }),
            vec![Effect::OpenEntry {
                project_id: ALPHA,
                started_at: at(6)
            }]
        );
    }

    #[test]
    fn the_first_pause_reason_wins_when_lock_is_followed_by_sleep() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::ScreenLocked { at: at(5) });
        assert!(t.handle(SystemEvent::WillSleep { at: at(6) }).is_empty());
        assert_eq!(t.paused().unwrap().reason, StopReason::Lock);
        assert_eq!(t.paused().unwrap().paused_at, at(5));
    }

    #[test]
    fn system_events_are_ignored_when_nothing_is_running() {
        let mut t = timer();
        assert!(t.handle(SystemEvent::WillSleep { at: at(1) }).is_empty());
        assert!(t.handle(SystemEvent::DidWake { at: at(2) }).is_empty());
        assert!(t.handle(SystemEvent::BecameActive { at: at(3) }).is_empty());
    }

    #[test]
    fn a_pause_never_ends_before_the_entry_started() {
        let mut t = timer();
        t.start(ALPHA, at(10));
        let effects = t.handle(SystemEvent::WentIdle { since: at(4) });
        assert_eq!(
            effects,
            vec![Effect::CloseEntry {
                ended_at: at(10),
                reason: StopReason::Idle
            }]
        );
    }

    #[test]
    fn starting_a_project_while_prompted_takes_the_prompt_down() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::WillSleep { at: at(10) });
        t.handle(SystemEvent::DidWake { at: at(70) });
        let effects = t.start(BETA, at(72));
        assert_eq!(
            effects,
            vec![
                Effect::CancelPrompt,
                Effect::OpenEntry {
                    project_id: BETA,
                    started_at: at(72)
                },
            ]
        );
        assert!(t.paused().is_none());
    }

    #[test]
    fn repeated_wakes_do_not_stack_prompts() {
        let mut t = timer();
        t.start(ALPHA, at(0));
        t.handle(SystemEvent::WillSleep { at: at(10) });
        assert_eq!(t.handle(SystemEvent::DidWake { at: at(70) }).len(), 1);
        assert!(t.handle(SystemEvent::BecameActive { at: at(71) }).is_empty());
    }

    #[test]
    fn a_session_restored_from_the_database_keeps_counting() {
        let session = Session {
            project_id: ALPHA,
            started_at: at(0),
        };
        let t = Timer::resumed_from(Some(session), Settings::default());
        assert!(t.is_running());
        assert_eq!(t.elapsed_seconds(at(30)), 1_800);
    }
}
