//! Background Discord Rich Presence worker.
//!
//! EuroScope callbacks run on its main thread and must never block. So the
//! plugin only ever calls [`Presence::update`], which enqueues the latest
//! controller state onto a channel and returns immediately. All the blocking
//! IPC work — connecting to Discord, (re)connecting when Discord starts later,
//! pushing activity updates — happens on the worker thread spawned here.

use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::Utc;
use discord_rich_presence::{
    DiscordIpc as _, DiscordIpcClient,
    activity::{Activity, ActivityType, Assets, Button, StatusDisplayType, Timestamps},
    error::Error as DiscordError,
};
use euroscope::{Context, ControllerRating, Facility};

/// Discord application (client) ID for the Rich Presence app.
const APP_ID: &str = "1525152891478872094";

/// An owned snapshot of the logged-in controller ("myself").
///
/// Not a EuroScope SDK type — it's our own comparable, thread-sendable bundle,
/// built from [`Context::controller_myself`]. Owned so the worker thread can
/// hold and diff it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct MyPosition {
    /// Controller callsign, e.g. `"LSGG_APP"`.
    pub callsign: String,
    /// Position identifier, e.g. `"GG"`.
    pub position_id: String,
    /// Primary frequency in MHz, e.g. `121.855`.
    pub primary_frequency: f64,
    /// Network rating.
    pub rating: ControllerRating,
    /// The kind of position occupied (observer vs a controlling facility).
    pub facility: Facility,
}

impl MyPosition {
    /// Snapshot the logged-in controller. `None` when not connected, when there
    /// is no valid controller, or when the callsign is empty (e.g. an observer
    /// that has not fully logged in).
    pub(crate) fn current(ctx: &Context) -> Option<Self> {
        if !ctx.is_connected() {
            return None;
        }
        let me = ctx.controller_myself()?;
        let callsign = me.callsign().to_owned();
        if callsign.is_empty() {
            return None;
        }
        Some(Self {
            callsign,
            position_id: me.position_id().to_owned(),
            primary_frequency: me.primary_frequency(),
            rating: me.rating(),
            facility: me.facility(),
        })
    }
}
/// How often the worker retries connecting / re-checks state while idle.
const RETRY_INTERVAL: Duration = Duration::from_secs(15);
/// Minimum spacing between presence pushes. Discord rate-limits Rich Presence
/// updates (~5 per 20 s); the "in range" count churns constantly, so we
/// coalesce bursts and never push more often than this.
const MIN_PUSH_INTERVAL: Duration = Duration::from_secs(5);

/// A full snapshot of what to display: the controller position plus live
/// traffic counts. Owned and comparable so the worker only pushes on change.
#[derive(Clone, PartialEq)]
pub(crate) struct Session {
    pub position: MyPosition,
    /// Aircraft currently tracked by me.
    pub tracked: u32,
    /// Aircraft currently in range.
    pub in_range: u32,
}

enum Msg {
    /// Latest session snapshot, or `None` when not connected to the network.
    Update(Option<Session>),
    /// Stop the worker and clear the presence.
    Shutdown,
}

/// Handle to the background presence worker. Dropping it shuts the worker down
/// and joins the thread.
pub(crate) struct Presence {
    tx: mpsc::Sender<Msg>,
    handle: Option<JoinHandle<()>>,
}

impl Presence {
    /// Spawn the worker thread.
    pub(crate) fn start() -> Self {
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || run(&rx));
        Self {
            tx,
            handle: Some(handle),
        }
    }

    /// Push the latest session snapshot (`None` when not connected).
    ///
    /// Non-blocking. Send errors (worker already gone) are ignored.
    pub(crate) fn update(&self, session: Option<Session>) {
        let _ = self.tx.send(Msg::Update(session));
    }
}

#[expect(clippy::missing_trait_methods, reason = "We don't use pin_drop")]
impl Drop for Presence {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Worker loop. Owns the Discord client; the main thread never touches it.
fn run(rx: &mpsc::Receiver<Msg>) {
    let mut client = DiscordIpcClient::new(APP_ID);
    let mut connected = false;
    // The state we want to show, and the state we last successfully pushed, so
    // we only hit Discord when something actually changed (it rate-limits).
    let mut desired: Option<Session> = None;
    let mut applied: Option<Option<Session>> = None;
    // Unix ms when the current controlling session began (for elapsed time).
    let mut session_start_ms: Option<i64> = None;
    // When we last pushed to Discord, for throttling.
    let mut last_push: Option<Instant> = None;

    loop {
        let pending = applied.as_ref() != Some(&desired);
        // If a change is pending but we pushed recently, wake exactly when the
        // throttle window elapses; otherwise idle until the next retry tick.
        let wait = match (pending, last_push) {
            (true, Some(t)) => MIN_PUSH_INTERVAL.saturating_sub(t.elapsed()),
            _ => RETRY_INTERVAL,
        };
        match rx.recv_timeout(wait) {
            Ok(Msg::Update(session)) => desired = session,
            Ok(Msg::Shutdown) | Err(RecvTimeoutError::Disconnected) => break,
            Err(RecvTimeoutError::Timeout) => {}
        }

        if !connected {
            match client.connect() {
                Ok(()) => {
                    connected = true;
                    applied = None; // force a fresh push on the new connection
                }
                // Discord isn't running yet; try again next interval.
                Err(_) => continue,
            }
        }

        // Nothing to do, or throttled: wait for the next wake.
        let throttled = last_push.is_some_and(|t| t.elapsed() < MIN_PUSH_INTERVAL);
        if applied.as_ref() == Some(&desired) || throttled {
            continue;
        }

        let result = if let Some(session) = &desired {
            if session_start_ms.is_none() {
                session_start_ms = Some(Utc::now().timestamp_millis());
            }
            set_presence(&mut client, session, session_start_ms)
        } else {
            session_start_ms = None;
            client.clear_activity()
        };

        if result.is_ok() {
            applied = Some(desired.clone());
            last_push = Some(Instant::now());
        } else {
            // Connection likely dropped; reconnect on the next loop.
            connected = false;
            let _ = client.close();
        }
    }

    let _ = client.close();
}

/// Build and push the Rich Presence activity for the current session.
fn set_presence(
    client: &mut DiscordIpcClient,
    session: &Session,
    start_ms: Option<i64>,
) -> Result<(), DiscordError> {
    let position = &session.position;
    let rating = position.rating.label();
    // Whether you're observing vs controlling depends on the facility you're
    // connected as, not your rating (an S3 can log in as an observer). When
    // observing, distinguish supervisors/admins by rating.
    let action = if position.facility.is_observer() {
        match position.rating {
            ControllerRating::Supervisor => "Supervising",
            ControllerRating::Administrator => "Administrating",
            _ => "Observing",
        }
    } else {
        "Controlling"
    };
    let controlling_as = if rating.is_empty() {
        format!("{} as {}", action, position.callsign)
    } else {
        format!("{} as {} ({rating})", action, position.callsign)
    };
    // A real primary frequency is in the VHF airband; EuroScope reports
    // 199.998 (and observers report 0) when there is none, so hide it then.
    let freq = position.primary_frequency;
    let has_frequency = (freq - 199.998_f64).abs() > 0.001_f64 && freq != 0.0_f64;
    let frequency = if has_frequency {
        format!("Frequency: {freq:.3} MHz")
    } else {
        "No primary frequency".to_owned()
    };
    let traffic = format!(
        "{} tracked · {} in range",
        session.tracked, session.in_range,
    );
    let state_line = if has_frequency {
        format!("{traffic} · {freq:.3} MHz")
    } else {
        traffic
    };
    let radar_url = format!("https://vatsim-radar.com/?atc={}", position.callsign);

    // The 1.x API takes `Into<Cow<str>>`, so owned `String`s move straight in —
    // no borrowed locals to keep alive.
    let mut activity = Activity::new()
        .name("Euroscope")
        .activity_type(ActivityType::Playing)
        .status_display_type(StatusDisplayType::Details)
        .details(&controlling_as)
        .details_url(&radar_url)
        .state(&state_line)
        .state_url(&radar_url)
        .assets(
            Assets::new()
                .large_image("https://risson.space/vacc-ch/logo.gif")
                .large_text(&controlling_as)
                .large_url(&radar_url)
                .small_image("vatsim")
                .small_text(&frequency)
                .small_url(&radar_url),
        )
        .buttons(vec![Button::new("See on VATSIM Radar", &radar_url)]);

    if let Some(start) = start_ms {
        activity = activity.timestamps(Timestamps::new().start(start));
    }

    client.set_activity(activity)
}
