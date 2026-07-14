use std::{
    sync::mpsc::{self, RecvTimeoutError},
    thread::{self, JoinHandle, sleep},
    time::{Duration, Instant},
};

use anyhow::Result;
use chrono::Utc;
use discord_rich_presence::{
    DiscordIpc as _, DiscordIpcClient,
    activity::{Activity, Assets, Button, Timestamps},
    error::Error as DiscordError,
};

use crate::{
    MainMsg,
    controller_information::ConnectionInformation,
    settings::Settings,
    template::{ActivityType, StatusDisplayType, Templates},
};

pub enum PresenceMsg {
    Update(ConnectionInformation),
    RefreshSettings(Settings, Templates),
    Ping,
    Shutdown,
}

pub struct Presence {
    presence_tx: mpsc::Sender<PresenceMsg>,
    handle: Option<JoinHandle<()>>,
}

impl Presence {
    pub fn start(
        main_tx: mpsc::Sender<MainMsg>,
        settings: Settings,
        templates: Templates,
        info: ConnectionInformation,
    ) -> Self {
        let (presence_tx, presence_rx) = mpsc::channel();
        let handle = Some(thread::spawn(move || {
            run(main_tx, &presence_rx, settings, templates, info)
        }));
        Self {
            presence_tx,
            handle,
        }
    }

    pub fn is_thread_dead(&self) -> bool {
        if let Some(handle) = &self.handle
            && handle.is_finished()
        {
            return true;
        }

        !self.presence_tx.send(PresenceMsg::Ping).is_ok()
    }

    pub fn send_update(&self, info: ConnectionInformation) -> bool {
        self.presence_tx.send(PresenceMsg::Update(info)).is_ok()
    }

    pub fn refresh_settings(&self, settings: Settings, templates: Templates) -> bool {
        self.presence_tx
            .send(PresenceMsg::RefreshSettings(settings, templates))
            .is_ok()
    }
}

#[expect(clippy::missing_trait_methods, reason = "We don't use pin_drop")]
impl Drop for Presence {
    fn drop(&mut self) {
        let _ = self.presence_tx.send(PresenceMsg::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

/// Worker loop. Owns the Discord client; the main thread never touches it.
fn run(
    main_tx: mpsc::Sender<MainMsg>,
    presence_rx: &mpsc::Receiver<PresenceMsg>,
    mut settings: Settings,
    mut templates: Templates,
    mut info: ConnectionInformation,
) {
    let mut client = DiscordIpcClient::new(settings.discord.client_id.clone());

    let mut start_time = now();
    let mut last_update =
        Instant::now() - Duration::from_secs(settings.general.activity_min_push_interval_s + 1);

    loop {
        match presence_rx.recv() {
            Ok(PresenceMsg::Update(i)) => {
                if i != info {
                    if i.label(&settings) != info.label(&settings) {
                        start_time = now();
                    }
                    info = i;
                    // TODO: handle error
                    if let Err(err) = set_activity(
                        &main_tx,
                        &settings,
                        &templates,
                        &mut client,
                        &info,
                        start_time,
                    ) {
                        let _ = main_tx.send(MainMsg::Log(format!(
                            "Failed to set Discord presence: {err}"
                        )));
                    }
                    last_update = Instant::now();
                }
            }
            Ok(PresenceMsg::RefreshSettings(s, t)) => {
                templates = t;
                settings = s;
                continue;
            }
            Ok(PresenceMsg::Shutdown) => break,
            Ok(PresenceMsg::Ping) => continue,
            Err(_) => {}
        }

        sleep(Duration::from_secs(
            settings.general.activity_min_push_interval_s,
        ));
    }

    let _ = client.close();
}

fn set_activity(
    main_tx: &mpsc::Sender<MainMsg>,
    settings: &Settings,
    templates: &Templates,
    client: &mut DiscordIpcClient,
    info: &ConnectionInformation,
    start_time: i64,
) -> Result<()> {
    let ctx = templates.make_context(&settings, &info)?;

    let render_string = |name| match templates.render(name, &ctx) {
        Ok(data) if !data.is_empty() => Some(data),
        Ok(_) => None,
        Err(err) => {
            let _ = main_tx.send(MainMsg::Log(format!(
                "Failed to render template `{name}`: {err}"
            )));
            None
        }
    };

    let name = render_string("name");
    let activity_type = match templates.render("activity_type", &ctx) {
        Ok(data) if !data.is_empty() => match serde_json::from_str::<ActivityType>(&data) {
            Ok(data) => Some(data),
            Err(_) => None,
        },
        Ok(_) => None,
        Err(err) => {
            let _ = main_tx.send(MainMsg::Log(format!(
                "Failed to render template `activity_type`: {err}"
            )));
            None
        }
    };
    let status_display_type = match templates.render("status_display_type", &ctx) {
        Ok(data) if !data.is_empty() => match serde_json::from_str::<StatusDisplayType>(&data) {
            Ok(data) => Some(data),
            Err(_) => None,
        },
        Ok(_) => None,
        Err(err) => {
            let _ = main_tx.send(MainMsg::Log(format!(
                "Failed to render template `status_display_type`: {err}"
            )));
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
    let buttons_first_url = render_string("buttons_first_label");
    let buttons_second_label = render_string("buttons_second_label");
    let buttons_second_url = render_string("buttons_second_label");

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

    client.set_activity(activity)?;

    Ok(())
}

fn now() -> i64 {
    Utc::now().timestamp_millis()
}
