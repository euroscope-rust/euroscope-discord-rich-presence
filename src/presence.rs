use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::{DateTime, Utc};
use discord_rich_presence::{
    DiscordIpc as _, DiscordIpcClient,
    activity::{Activity, Assets, Button, Timestamps},
};
use tracing::{debug, error, info, trace, warn};

use crate::{
    controller_information::ConnectionInformation,
    settings::Settings,
    status::{self, SharedStatus, Status},
    templates::{ActivityType, StatusDisplayType, Templates},
    utils::now,
};

#[derive(Debug)]
pub enum PresenceMsg {
    Update(Option<ConnectionInformation>),
    // Boxed to keep the enum small
    RefreshSettings(Box<(Settings, Templates)>),
    /// Resume (`true`) or stop (`false`) publishing to Discord. While stopped
    /// the worker holds no Discord connection, and the main thread sends it no
    /// updates.
    SetProcessing(bool),
    Shutdown,
}

/// What the worker wants Discord to be showing.
#[derive(PartialEq)]
enum Target {
    /// Clear the activity entirely (idle while idle presence is disabled).
    Clear,
    /// Show the rendered activity for this state.
    Show(ConnectionInformation),
}

/// Worker loop. Owns the Discord client; the main thread never touches it.
///
/// The loop coalesces updates: it never pushes to Discord more than once per
/// `activity_min_push_interval_s`, and — so the presence stays fresh and the
/// idle tag line keeps rotating — pushes at least once per
/// `activity_max_push_interval_s`. It connects to Discord only while there is
/// something to show and disconnects whenever there isn't (an idle state with
/// idle presence disabled, or a connection type we don't publish), so an
/// instance with nothing to show doesn't hold a Discord pipe and clobber
/// another instance sharing the same application. On a dropped connection it
/// reconnects at most once per `activity_retry_interval_s`.
///
/// The first iteration always blocks on `recv`, so nothing is touched before
/// the main thread hands us a state.
///
/// Every iteration ends by publishing a snapshot of what we're doing into
/// `status`, so `.drp status` can report on us without interrupting us.
#[expect(clippy::too_many_lines, reason = "Single cohesive worker loop")]
pub fn run(
    presence_rx: &mpsc::Receiver<PresenceMsg>,
    status: &SharedStatus,
    mut settings: Settings,
    mut templates: Templates,
) {
    info!("Starting Discord presence update thread.");

    let mut client = DiscordIpcClient::new(settings.discord.client_id.clone());

    // The latest state from the main thread, or `None` when there's nothing to
    // show. `ready` stays false until the first message, so we touch nothing
    // before then.
    let mut info: Option<ConnectionInformation> = None;
    let mut ready = false;

    // Cleared by `.drp stop`, which parks us with nothing to show — and so, by
    // the logic below, with no Discord connection either.
    let mut processing = true;

    // We connect lazily — only while there is something to show — and drop the
    // connection otherwise, so an instance with nothing to show doesn't hold a
    // Discord pipe. `last_connect` is the failure backoff timer (only bumped on
    // a failed attempt); seeding it in the past lets the first connect happen
    // immediately.
    let mut connected = false;
    let mut last_connect = Instant::now()
        .checked_sub(Duration::from_secs(
            settings.general.activity_retry_interval_s,
        ))
        .unwrap_or_else(Instant::now);

    let mut start_time = now();
    // Seed the last push far enough in the past that the first push isn't
    // delayed by the min-push window.
    let mut last_push = Instant::now()
        .checked_sub(Duration::from_secs(
            settings.general.activity_min_push_interval_s,
        ))
        .unwrap_or_else(Instant::now);
    // What Discord is currently showing, or `None` if we've never applied a
    // target.
    let mut published: Option<Target> = None;

    // Wall clock counterparts of `last_push`, kept only for `.drp status`.
    let mut last_push_at: Option<DateTime<Utc>> = None;
    let mut last_payload: Option<String> = None;

    loop {
        let retry_interval = Duration::from_secs(settings.general.activity_retry_interval_s);
        let target = current_target(info.as_ref(), &settings, processing);

        // How long to block for the next message before acting on our own:
        //  - not ready → await the first state;
        //  - nothing to show → already disconnected, block until a message;
        //  - showing but disconnected → wake to (re)connect;
        //  - showing and connected → wake when the next push is due.
        let wait = if ready {
            match &target {
                Target::Clear => None,
                Target::Show(_) if connected => {
                    next_push_interval(published.as_ref(), &target, &settings)
                        .map(|interval| interval.saturating_sub(last_push.elapsed()))
                }
                Target::Show(_) => Some(retry_interval.saturating_sub(last_connect.elapsed())),
            }
        } else {
            None
        };

        let msg = match wait {
            None => {
                if let Ok(msg) = presence_rx.recv() {
                    Some(msg)
                } else {
                    error!("Failed to recv from the main thread. Shutting down Discord thread.");
                    break;
                }
            }
            Some(wait) => match presence_rx.recv_timeout(wait) {
                Ok(msg) => Some(msg),
                // Timed out: fall through to the (re)connect / push logic below.
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => {
                    error!("Failed to recv from the main thread. Shutting down Discord thread.");
                    break;
                }
            },
        };
        trace!(?msg, "Received message from main thread.");

        match msg {
            Some(PresenceMsg::Update(i)) => {
                let previous = display_label(info.as_ref(), &settings);
                let new = display_label(i.as_ref(), &settings);
                if previous != new {
                    start_time = now();
                    // `ready` skips the initial none-to-first-state jump.
                    if ready {
                        debug!(
                            from = previous,
                            to = new,
                            "Controller connection type changed."
                        );
                    }
                }
                info = i;
                ready = true;
            }
            Some(PresenceMsg::RefreshSettings(boxed)) => {
                let (s, t) = *boxed;
                // A changed client id makes the current connection useless.
                if s.discord.client_id != settings.discord.client_id {
                    let _ = client.close();
                    client = DiscordIpcClient::new(s.discord.client_id.clone());
                    connected = false;
                }
                settings = s;
                templates = t;
                // Force a re-render with the new templates by treating the
                // current presence as unknown.
                published = None;
            }
            Some(PresenceMsg::SetProcessing(wanted)) => {
                if wanted != processing {
                    info!(
                        processing = wanted,
                        "Processing toggled by the main thread."
                    );
                }
                processing = wanted;
            }
            Some(PresenceMsg::Shutdown) => break,
            // A timeout just drops us into the (re)connect / push logic below.
            None => {}
        }

        let target = current_target(info.as_ref(), &settings, processing);
        if ready {
            match &target {
                // Nothing to show: drop the connection so another instance
                // sharing this Discord application can take over the presence.
                Target::Clear => {
                    if connected {
                        debug!("Nothing to show; disconnecting from Discord.");
                        let _ = client.close();
                        connected = false;
                    }
                    published = Some(Target::Clear);
                }
                Target::Show(state) => {
                    // Connect on demand; back off to the retry interval only
                    // after a failed attempt.
                    if !connected && last_connect.elapsed() >= retry_interval {
                        connected = connect(&mut client);
                        if !connected {
                            last_connect = Instant::now();
                        }
                    }

                    // Push once the applicable window (min after a change, max
                    // as a heartbeat) has elapsed.
                    if connected
                        && let Some(interval) =
                            next_push_interval(published.as_ref(), &target, &settings)
                        && last_push.elapsed() >= interval
                    {
                        match set_activity(
                            &settings,
                            &mut templates,
                            &mut client,
                            state,
                            start_time,
                        ) {
                            Ok(payload) => {
                                published = Some(Target::Show(state.clone()));
                                last_push = Instant::now();
                                last_push_at = Some(Utc::now());
                                last_payload = Some(payload);
                            }
                            Err(err) => {
                                warn!(%err, "Failed to set Discord presence.");
                                // The write failed, so assume the pipe dropped
                                // and reconnect on the retry interval.
                                connected = false;
                            }
                        }
                    }
                }
            }
        }

        // A push that is already due but that we couldn't make (we're
        // disconnected, or the loop is about to make it) saturates to now
        // rather than reading as unscheduled.
        let next_push = if ready && matches!(target, Target::Show(_)) {
            next_push_interval(published.as_ref(), &target, &settings).and_then(|interval| {
                status::in_duration(interval.saturating_sub(last_push.elapsed()))
            })
        } else {
            None
        };
        status.set(Status {
            connected,
            state: ready.then(|| display_label(info.as_ref(), &settings)),
            last_push: last_push_at,
            next_push,
            last_payload: last_payload.clone(),
        });
    }

    let _ = client.close();
}

/// What Discord should currently be showing: nothing while processing is
/// stopped, when there's no state, or while idle if idle presence is disabled;
/// otherwise the latest state.
fn current_target(
    info: Option<&ConnectionInformation>,
    settings: &Settings,
    processing: bool,
) -> Target {
    if !processing {
        return Target::Clear;
    }
    match info {
        Some(ConnectionInformation::Idle) if !settings.idle.set_presence_when_idle => Target::Clear,
        Some(info) => Target::Show(info.clone()),
        None => Target::Clear,
    }
}

/// A short label for the current state, for logging. `None` (nothing to show)
/// renders as `"None"`.
fn display_label(info: Option<&ConnectionInformation>, settings: &Settings) -> &'static str {
    info.map_or("None", |info| info.label(settings))
}

/// The window that must elapse after the last push before `target` should be
/// (re)applied to Discord, or `None` when it already matches and needs no
/// heartbeat.
///
/// A target that differs from what Discord shows is applied after the short
/// `activity_min_push_interval_s` (coalescing bursts of changes). A matching
/// shown state is still re-pushed after `activity_max_push_interval_s` as a
/// heartbeat that keeps the presence fresh and rotates the idle tag line; a
/// cleared presence needs no heartbeat.
fn next_push_interval(
    published: Option<&Target>,
    target: &Target,
    settings: &Settings,
) -> Option<Duration> {
    if published != Some(target) {
        Some(Duration::from_secs(
            settings.general.activity_min_push_interval_s,
        ))
    } else if matches!(target, Target::Show(_)) {
        Some(Duration::from_secs(
            settings.general.activity_max_push_interval_s,
        ))
    } else {
        None
    }
}

#[inline]
fn connect(client: &mut DiscordIpcClient) -> bool {
    match client.connect() {
        Ok(()) => true,
        Err(err) => {
            warn!(%err, "Failed to connect to Discord.");
            false
        }
    }
}

/// Push `info` to Discord, returning the activity payload we sent so
/// `.drp status` can show the user what went out.
#[inline]
fn set_activity(
    settings: &Settings,
    templates: &mut Templates,
    client: &mut DiscordIpcClient,
    info: &ConnectionInformation,
    start_time: i64,
) -> Result<String> {
    let activity = make_activity(settings, templates, info, start_time)?;
    let payload = serde_json::to_string(&activity)?;

    client.set_activity(activity)?;

    // Discord replies to SET_ACTIVITY with a frame describing the result. The
    // crate's `set_activity` never reads it, so a rejected payload otherwise
    // looks like success; surface it so we can see accept/reject and why.
    let (_, response) = client.recv()?;
    debug!(%response, "Discord SET_ACTIVITY response");

    Ok(payload)
}

#[inline]
#[expect(clippy::too_many_lines, reason = "Couldn't care less.")]
fn make_activity<'a>(
    settings: &'a Settings,
    templates: &'a mut Templates,
    info: &'a ConnectionInformation,
    start_time: i64,
) -> Result<Activity<'a>> {
    let ctx = templates.make_context(settings, info, start_time)?;

    let render_string = |name| match templates.render(name, &ctx) {
        Ok(data) if !data.is_empty() => Some(data),
        Ok(_) => None,
        Err(err) => {
            warn!(%err, template_name = name, "Failed to render template.");
            None
        }
    };

    let name = render_string("name");
    let activity_type = match templates.render("activity_type", &ctx) {
        Ok(data) if !data.is_empty() => serde_json::from_str::<ActivityType>(&data).ok(),
        Ok(_) => None,
        Err(err) => {
            warn!(%err, template_name = "activity_type", "Failed to render template.");
            None
        }
    };
    let status_display_type = match templates.render("status_display_type", &ctx) {
        Ok(data) if !data.is_empty() => serde_json::from_str::<StatusDisplayType>(&data).ok(),
        Ok(_) => None,
        Err(err) => {
            warn!(%err, template_name = "status_display_type", "Failed to render template.");
            None
        }
    };
    let details = render_string("details");
    let details_url = render_string("details_url");
    let state = render_string("state");
    let state_url = render_string("state_url");

    let mut activity = Activity::new();
    if let Some(name) = name {
        activity = activity.name(name);
    }
    if let Some(activity_type) = activity_type {
        activity = activity.activity_type(activity_type.into());
    }
    if let Some(status_display_type) = status_display_type {
        activity = activity.status_display_type(status_display_type.into());
    }
    if let Some(details) = details {
        activity = activity.details(details);
    }
    if let Some(details_url) = details_url {
        activity = activity.details_url(details_url);
    }
    if let Some(state) = state {
        activity = activity.state(state);
    }
    if let Some(state_url) = state_url {
        activity = activity.state_url(state_url);
    }

    let large_image = render_string("assets_large_image");
    let large_text = render_string("assets_large_text");
    let large_url = render_string("assets_large_url");
    let small_image = render_string("assets_small_image");
    let small_text = render_string("assets_small_text");
    let small_url = render_string("assets_small_url");

    let mut assets = Assets::new();
    if let Some(large_image) = large_image {
        assets = assets.large_image(large_image);
    }
    if let Some(large_text) = large_text {
        assets = assets.large_text(large_text);
    }
    if let Some(large_url) = large_url {
        assets = assets.large_url(large_url);
    }
    if let Some(small_image) = small_image {
        assets = assets.small_image(small_image);
    }
    if let Some(small_text) = small_text {
        assets = assets.small_text(small_text);
    }
    if let Some(small_url) = small_url {
        assets = assets.small_url(small_url);
    }
    activity = activity.assets(assets);

    let buttons_first_label = render_string("buttons_first_label");
    let buttons_first_url = render_string("buttons_first_url");
    let buttons_second_label = render_string("buttons_second_label");
    let buttons_second_url = render_string("buttons_second_url");

    let make_button = |label: Option<String>, url: Option<String>| {
        if let Some(label) = label
            && let Some(url) = url
            && label.len() <= 32
            && url.len() <= 512
        {
            Some(Button::new(label, url))
        } else {
            None
        }
    };

    let mut buttons = Vec::with_capacity(2);
    if let Some(button) = make_button(buttons_first_label, buttons_first_url) {
        buttons.push(button);
    }
    if let Some(button) = make_button(buttons_second_label, buttons_second_url) {
        buttons.push(button);
    }
    activity = activity.buttons(buttons);

    activity = activity.timestamps(Timestamps::new().start(start_time));

    Ok(activity)
}

#[cfg(test)]
mod tests {
    use super::{Target, current_target, make_activity};
    use crate::{
        controller_information::ConnectionInformation, settings::Settings, templates::Templates,
        utils::now,
    };

    #[test]
    fn stopped_shows_nothing() {
        let settings = Settings::load(&[]).expect("settings");
        let info = ConnectionInformation::Idle;

        assert!(current_target(Some(&info), &settings, true) != Target::Clear);
        assert!(current_target(Some(&info), &settings, false) == Target::Clear);
    }

    #[test]
    fn make_activity_idle() {
        let settings = Settings::load(&[]).expect("settings");
        let mut templates = Templates::new(&settings).expect("templates");
        let start_time = now();

        let info = ConnectionInformation::Idle;

        make_activity(&settings, &mut templates, &info, start_time).expect("activity");
    }
}
