use std::{
    sync::mpsc,
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Utc;
use discord_rich_presence::{
    DiscordIpc as _, DiscordIpcClient,
    activity::{Activity, Assets, Button, Timestamps},
};
use tracing::{debug, error, info, warn};

use crate::{
    controller_information::ConnectionInformation,
    settings::Settings,
    templates::{ActivityType, StatusDisplayType, Templates},
};

pub enum PresenceMsg {
    Update(ConnectionInformation),
    // Boxed to keep the enum small
    RefreshSettings(Box<(Settings, Templates)>),
    Shutdown,
}

/// Worker loop. Owns the Discord client; the main thread never touches it.
///
/// The loop coalesces updates: it never pushes to Discord more than once per
/// `activity_min_push_interval_s`, always sending the most recent state when
/// the window elapses. If the connection to Discord drops, it reconnects at
/// most once per `activity_retry_interval_s`.
///
/// The first iteration always blocks on `recv`, so nothing is published before
/// the main thread hands us a state to show. Connecting up front is allowed and
/// only makes that first push faster.
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
    // The state Discord is currently showing. `None` until we've pushed once,
    // so the first received state is always published, even if it matches the
    // state we were seeded with.
    let mut published: Option<ConnectionInformation> = None;
    let mut dirty = false;

    loop {
        let retry_interval = Duration::from_secs(settings.general.activity_retry_interval_s);
        let min_push_interval = Duration::from_secs(settings.general.activity_min_push_interval_s);

        // How long to wait for the next message. When we owe a push we wake
        // ourselves to send it: after the min-push window if connected, or after
        // the retry window if we still need to (re)connect. With nothing owed we
        // block until a message arrives.
        let wait = if !dirty {
            None
        } else if connected {
            Some(min_push_interval.saturating_sub(last_push.elapsed()))
        } else {
            Some(retry_interval.saturating_sub(last_connect.elapsed()))
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
                if i.label(&settings) != info.label(&settings) {
                    start_time = now();
                }
                info = i;
                // Owe a push whenever the latest state differs from what Discord
                // is actually showing (not merely from the previous receive).
                dirty = published.as_ref() != Some(&info);
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
                // Re-render and re-push with the new templates.
                dirty = true;
            }
            Some(PresenceMsg::Shutdown) => break,
            // A timeout just drops us into the (re)connect / push logic below.
            None => {}
        }

        if !dirty {
            continue;
        }

        // (Re)connect if needed, no more than once per retry interval.
        if !connected && last_connect.elapsed() >= retry_interval {
            connected = connect(&mut client);
            last_connect = Instant::now();
            if !connected {
                // Still down; the retry interval governs the next attempt.
                continue;
            }
        }

        // Push the latest state once the min-push window has elapsed.
        if last_push.elapsed() >= min_push_interval {
            match set_activity(&settings, &mut templates, &mut client, &info, start_time) {
                Ok(()) => {
                    dirty = false;
                    published = Some(info.clone());
                    last_push = Instant::now();
                }
                Err(err) => {
                    warn!(%err, "Failed to set Discord presence.");
                    // The write failed, so assume the pipe dropped and let the
                    // retry interval govern the next reconnect attempt.
                    connected = false;
                }
            }
        }
    }

    let _ = client.close();
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
#[expect(clippy::too_many_lines, reason = "Couldn't care less.")]
fn make_activity<'a>(
    settings: &'a Settings,
    templates: &'a mut Templates,
    info: &'a ConnectionInformation,
    start_time: i64,
) -> Result<Activity<'a>> {
    let ctx = templates.make_context(settings, info)?;

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

#[inline]
fn now() -> i64 {
    Utc::now().timestamp_millis()
}

#[cfg(test)]
mod tests {
    use super::{make_activity, now};
    use crate::{
        controller_information::ConnectionInformation, settings::Settings, templates::Templates,
    };

    #[test]
    fn make_activity_idle() {
        let settings = Settings::load(&[]).expect("settings");
        let templates = Templates::new(&settings).expect("templates");
        let start_time = now();

        let info = ConnectionInformation::Idle;

        make_activity(&settings, &templates, &info, start_time).expect("activity");
    }
}
