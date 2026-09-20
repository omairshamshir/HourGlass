//! Fill a scratch database with a plausible few days of work.
//!
//! Used for looking at the app with real-shaped data in it:
//!
//! ```text
//! cargo run -p hourglass-store --example seed -- /tmp/hourglass-demo.db
//! HOURGLASS_DB=/tmp/hourglass-demo.db cargo run -p hourglass
//! ```

use chrono::{Duration, Local, TimeZone, Utc};
use hourglass_core::model::{ProjectId, StopReason};
use hourglass_store::Store;
use std::path::PathBuf;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp/hourglass-demo.db"));

    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path).expect("scratch database opens");

    let atlas = store.create_project("Atlas Redesign", 0).unwrap().id;
    let beacon = store.create_project("Beacon API", 1).unwrap().id;
    let cirrus = store.create_project("Cirrus Migration", 2).unwrap().id;
    let internal = store.create_project("Internal", 6).unwrap().id;

    // Local hour on a given day offset from today.
    let at = |days_ago: i64, hour: u32, minute: u32| {
        let day = (Local::now() - Duration::days(days_ago)).date_naive();
        let naive = day.and_hms_opt(hour, minute, 0).unwrap();
        Local
            .from_local_datetime(&naive)
            .earliest()
            .unwrap()
            .with_timezone(&Utc)
    };

    let record = |project: ProjectId,
                  days_ago: i64,
                  hour: u32,
                  minute: u32,
                  minutes: i64,
                  reason: StopReason| {
        let start = at(days_ago, hour, minute);
        store.open_entry(project, start).unwrap();
        store
            .close_open_entry(start + Duration::minutes(minutes), reason)
            .unwrap();
    };

    // Today: a morning block, a meeting, a long afternoon, one idle stretch.
    record(atlas, 0, 9, 12, 92, StopReason::Switch);
    record(internal, 0, 10, 44, 34, StopReason::Switch);
    record(beacon, 0, 11, 18, 47, StopReason::Idle);
    record(atlas, 0, 13, 30, 118, StopReason::Manual);
    record(cirrus, 0, 15, 40, 65, StopReason::Lock);

    // Earlier in the week, so the week and month ranges have something to show.
    for days_ago in 1..=5 {
        record(atlas, days_ago, 9, 30, 150, StopReason::Manual);
        record(beacon, days_ago, 13, 0, 95, StopReason::Sleep);
        if days_ago % 2 == 0 {
            record(cirrus, days_ago, 16, 0, 80, StopReason::Manual);
        }
    }

    println!("Seeded {}", path.display());
}
