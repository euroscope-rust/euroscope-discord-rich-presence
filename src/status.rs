//! A snapshot of what the presence worker is doing, shared with the main
//! thread so `.drp status` can report on it without interrupting the worker.

use std::{
    sync::{Mutex, PoisonError},
    time::Duration,
};

use chrono::{DateTime, TimeDelta, Utc};

/// What the presence worker last did, and what it intends to do next.
///
/// Published by the worker at the end of every loop iteration and read by the
/// main thread when the user asks for `.drp status`. It carries only what the
/// worker alone knows — whether processing is stopped is the main thread's
/// business, since that's where the commands land.
#[derive(Clone, Debug, Default)]
pub struct Status {
    /// Whether we currently hold a connection to Discord.
    pub connected: bool,
    /// Label of the connection state last received from the main thread, or
    /// `None` until the first one arrives.
    pub state: Option<&'static str>,
    /// When the last activity was pushed to Discord.
    pub last_push: Option<DateTime<Utc>>,
    /// When the next push is due, or `None` when none is.
    pub next_push: Option<DateTime<Utc>>,
    /// The activity payload of the last successful push.
    pub last_payload: Option<String>,
}

impl Status {
    /// The snapshot as message box lines, one field per line.
    pub fn report(&self) -> Vec<String> {
        vec![
            format!(
                "  Discord: {}",
                if self.connected {
                    "connected"
                } else {
                    "not connected"
                }
            ),
            format!("  Connection: {}", self.state.unwrap_or("unknown")),
            format!(
                "  Last update: {}",
                self.last_push
                    .map_or_else(|| "never".to_owned(), format_time)
            ),
            format!(
                "  Next update: {}",
                self.next_push
                    .map_or_else(|| "none scheduled".to_owned(), format_time)
            ),
            format!(
                "  Last data sent: {}",
                self.last_payload.as_deref().unwrap_or("none")
            ),
        ]
    }
}

/// A [`Status`] behind a lock, shared between the main thread and the worker.
///
/// A poisoned lock is recovered from rather than propagated: a half-written
/// status is not worth taking the plugin down over.
#[derive(Debug, Default)]
pub struct SharedStatus(Mutex<Status>);

impl SharedStatus {
    /// The latest snapshot published by the worker.
    pub fn get(&self) -> Status {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Publish a new snapshot.
    pub fn set(&self, status: Status) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = status;
    }
}

/// The wall clock instant `duration` from now, or `None` if it doesn't fit in a
/// [`DateTime`].
pub fn in_duration(duration: Duration) -> Option<DateTime<Utc>> {
    let delta = TimeDelta::from_std(duration).ok()?;
    Utc::now().checked_add_signed(delta)
}

/// An absolute time plus how far away it is, e.g.
/// `2026-08-24 12:34:56Z (5s ago)`.
fn format_time(time: DateTime<Utc>) -> String {
    // Rounded, not truncated: a push due in a whisker under 30s should read as
    // `in 30s`, not as `in 29s`.
    let millis = time.signed_duration_since(Utc::now()).num_milliseconds();
    let half_second = if millis < 0 { -500_i64 } else { 500_i64 };
    let seconds = millis.saturating_add(half_second) / 1000_i64;

    let relative = if seconds < 0_i64 {
        format!("{}s ago", seconds.unsigned_abs())
    } else {
        format!("in {seconds}s")
    };
    format!("{} ({relative})", time.format("%Y-%m-%d %H:%M:%SZ"))
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::{SharedStatus, Status, in_duration};

    #[test]
    fn report_without_data() {
        let report = Status::default().report();
        assert!(report.iter().any(|line| line.contains("not connected")));
        assert!(report.iter().any(|line| line.contains("never")));
    }

    #[test]
    fn report_with_data() {
        let status = Status {
            connected: true,
            state: Some("Connected"),
            last_push: in_duration(Duration::ZERO),
            next_push: in_duration(Duration::from_secs(30)),
            last_payload: Some(r#"{"state":"LSGG_APP"}"#.to_owned()),
        };
        let report = status.report();
        assert!(report.iter().any(|line| line.contains("in 30s")));
        assert!(report.iter().any(|line| line.contains("LSGG_APP")));
    }

    #[test]
    fn shared_round_trip() {
        let shared = SharedStatus::default();
        assert!(!shared.get().connected);
        shared.set(Status {
            connected: true,
            ..Status::default()
        });
        assert!(shared.get().connected);
    }
}
