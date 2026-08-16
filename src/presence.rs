use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Result;
use discord_rich_presence::{
    DiscordIpc as _, DiscordIpcClient,
    activity::{Activity, Assets, Button, Timestamps},
};
use tracing::{debug, error, info, warn};

use crate::{
    controller_information::ConnectionInformation,
    settings::Settings,
    templates::{ActivityType, StatusDisplayType, Templates},
    utils::now,
};

pub enum PresenceMsg {
    Update(ConnectionInformation),
    // Boxed to keep the enum small
    RefreshSettings(Box<(Settings, Templates)>),
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
/// `activity_max_push_interval_s`. While idle with idle presence disabled it
/// clears the activity instead. If the connection to Discord drops, it
/// reconnects at most once per `activity_retry_interval_s`.
///
/// The first iteration always blocks on `recv`, so nothing is published before
/// the main thread hands us a state to show. Connecting up front is allowed and
/// only makes that first push faster.
#[expect(clippy::too_many_lines, reason = "Single cohesive worker loop")]
pub fn run(
    presence_rx: &mpsc::Receiver<PresenceMsg>,
    mut settings: Settings,
    mut templates: Templates,
) {
    info!("Starting Discord presence update thread.");

    let mut client = DiscordIpcClient::new(settings.discord.client_id.clone());

    // Placeholder until the first update arrives; never published on its own
    // (see `published` below).
    let mut info = ConnectionInformation::default();

    // Connect ahead of the first message so the first push is immediate. A
    // failure here is fine; we retry inside the loop on the retry interval.
    let mut connected = connect(&mut client);
    let mut last_connect = Instant::now();

    let mut start_time = now();
    // Seed the last push far enough in the past that the first update is sent
    // without waiting on the min-push interval.
    let mut last_push = Instant::now()
        .checked_sub(Duration::from_secs(
            settings.general.activity_min_push_interval_s,
        ))
        .unwrap_or_else(Instant::now);
    // What Discord is currently showing, or `None` if we've never pushed.
    let mut published: Option<Target> = None;
    // Whether the main thread has handed us a state yet. Until it has we block
    // on `recv` and publish nothing.
    let mut ready = false;

    loop {
        let retry_interval = Duration::from_secs(settings.general.activity_retry_interval_s);

        // What Discord should show right now, and the window that must elapse
        // after the last push before we (re)apply it (or `None` if it already
        // matches and needs no heartbeat).
        let target = current_target(&info, &settings);
        let push_due = next_push_interval(published.as_ref(), &target, &settings);

        // How long to block for the next message before acting on our own. Not
        // ready → await the first state. Otherwise, if a push is owed, wake to
        // (re)connect or to send it; if not, block until a message arrives.
        let wait = if !ready {
            None
        } else if !connected {
            push_due.map(|_| retry_interval.saturating_sub(last_connect.elapsed()))
        } else {
            push_due.map(|interval| interval.saturating_sub(last_push.elapsed()))
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

        match msg {
            Some(PresenceMsg::Update(i)) => {
                let previous = info.label(&settings);
                let new = i.label(&settings);
                if previous != new {
                    start_time = now();
                    debug!(
                        from = previous,
                        to = new,
                        "Controller connection type changed."
                    );
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
            Some(PresenceMsg::Shutdown) => break,
            // A timeout just drops us into the (re)connect / push logic below.
            None => {}
        }

        if !ready {
            continue;
        }

        // (Re)connect if needed, no more than once per retry interval.
        if !connected && last_connect.elapsed() >= retry_interval {
            connected = connect(&mut client);
            last_connect = Instant::now();
        }
        if !connected {
            // Still down; the retry interval governs the next attempt.
            continue;
        }

        // Apply the target once its window (min after a change, max as a
        // heartbeat) has elapsed.
        let target = current_target(&info, &settings);
        if let Some(interval) = next_push_interval(published.as_ref(), &target, &settings)
            && last_push.elapsed() >= interval
        {
            let result = match &target {
                Target::Show(state) => {
                    set_activity(&settings, &mut templates, &mut client, state, start_time)
                }
                Target::Clear => clear_activity(&mut client),
            };
            match result {
                Ok(()) => {
                    published = Some(target);
                    last_push = Instant::now();
                }
                Err(err) => {
                    warn!(%err, "Failed to update Discord presence.");
                    // The write failed, so assume the pipe dropped and let the
                    // retry interval govern the next reconnect attempt.
                    connected = false;
                }
            }
        }
    }

    let _ = client.close();
}

/// What Discord should currently be showing: nothing while idle if idle
/// presence is disabled, otherwise the latest state.
fn current_target(info: &ConnectionInformation, settings: &Settings) -> Target {
    if !settings.idle.set_presence_when_idle && matches!(info, ConnectionInformation::Idle) {
        Target::Clear
    } else {
        Target::Show(info.clone())
    }
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

#[inline]
fn set_activity(
    settings: &Settings,
    templates: &mut Templates,
    client: &mut DiscordIpcClient,
    info: &ConnectionInformation,
    start_time: i64,
) -> Result<()> {
    let activity = make_activity(settings, templates, info, start_time)?;

    client.set_activity(activity)?;

    // Discord replies to SET_ACTIVITY with a frame describing the result. The
    // crate's `set_activity` never reads it, so a rejected payload otherwise
    // looks like success; surface it so we can see accept/reject and why.
    let (_, response) = client.recv()?;
    debug!(%response, "Discord SET_ACTIVITY response");

    Ok(())
}

#[inline]
fn clear_activity(client: &mut DiscordIpcClient) -> Result<()> {
    client.clear_activity()?;

    // Like `set_activity`, read Discord's reply so it doesn't desync the pipe.
    let (_, response) = client.recv()?;
    debug!(%response, "Discord clear-activity response");

    Ok(())
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
    use super::make_activity;
    use crate::{
        controller_information::ConnectionInformation, settings::Settings, templates::Templates,
        utils::now,
    };

    #[test]
    fn make_activity_idle() {
        let settings = Settings::load(&[]).expect("settings");
        let mut templates = Templates::new(&settings).expect("templates");
        let start_time = now();

        let info = ConnectionInformation::Idle;

        make_activity(&settings, &mut templates, &info, start_time).expect("activity");
    }
}
