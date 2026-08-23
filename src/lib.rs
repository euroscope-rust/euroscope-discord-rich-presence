#![doc = include_str!("../README.md")]

pub mod controller_information;
pub mod presence;
pub mod settings;
pub mod templates;
pub mod tracing;
pub mod utils;

use std::{fs::metadata, path::PathBuf, sync::mpsc, thread, thread::JoinHandle};

use ::tracing::{error, info, trace, warn};
use euroscope::{Context, Plugin, get_plugin_path, register_plugin};

use crate::{
    controller_information::ConnectionInformation,
    presence::{PresenceMsg, run},
    settings::Settings,
    templates::Templates,
    tracing::{LogReloadHandle, reload_log_level},
};

struct DiscordRichPresence {
    presence_tx: mpsc::Sender<PresenceMsg>,
    handle: Option<JoinHandle<()>>,
    settings_path: Option<PathBuf>,
    thread_seen_dead: bool,
    log_reload_handle: LogReloadHandle,
}

impl DiscordRichPresence {
    fn make_settings_path() -> Option<PathBuf> {
        if let Some(mut path) = get_plugin_path() {
            path.set_extension("toml");
            if metadata(&path).is_ok_and(|m| m.is_file()) {
                info!(target: "mbox", path = %path.display(), "Found settings file.");
                Some(path)
            } else {
                warn!(target: "mbox", path = %path.display(), "No settings file found.");
                None
            }
        } else {
            error!(target: "mbox", "Failed to retrieve the plugin path. This is a bug, please report it.");
            None
        }
    }

    fn send_presence_msg(&mut self, msg: PresenceMsg) {
        if self.presence_tx.send(msg).is_err() {
            error!(target: "mbox", "Discord presence update thread has died. This is a bug, please report it.");
            self.thread_seen_dead = true;
        }
    }
}

#[expect(clippy::missing_trait_methods, reason = "We don't use pin_drop")]
impl Drop for DiscordRichPresence {
    fn drop(&mut self) {
        let _ = self.presence_tx.send(PresenceMsg::Shutdown);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

#[expect(
    clippy::missing_trait_methods,
    reason = "We don't need all the callbacks"
)]
impl Plugin for DiscordRichPresence {
    const AUTHOR: &'static str = "Marc Schmitt <vatsim@risson.space>";
    const COPYRIGHT: &'static str = "EUPL-1.2";
    const NAME: &'static str = "Discord Rich Presence";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    fn new(_ctx: &mut Context) -> Self {
        let tracing_crude = tracing::install_crude();
        info!(target: "mbox", version = Self::VERSION, "{} loaded.", Self::NAME);

        let settings_path = Self::make_settings_path();

        let mut settings = if let Some(plugin_path) = &settings_path {
            let plugin_path = plugin_path.display().to_string();
            info!(target: "mbox", path = %plugin_path, "Loading settings...");
            Settings::load(&[&plugin_path]).unwrap_or_else(|err| {
                warn!(target: "mbox", %err, "Unable to load settings. Running with default settings.");
                Settings::load(&[]).expect("to load default settings.")
            })
        } else {
            warn!(target: "mbox", "Unable to find settings file. Running with default settings.");
            Settings::load(&[]).expect("to load default settings.")
        };

        let templates = match Templates::new(&settings) {
            Ok(templates) => templates,
            Err(err) => {
                warn!(target: "mbox", %err, "Failed to load templates. Falling back to default settings.");
                settings = Settings::load(&[]).expect("to load default settings.");
                Templates::new(&settings).expect("to load default templates.")
            }
        };

        let log_reload_handle = tracing::install(&settings);
        drop(tracing_crude);

        let (presence_tx, presence_rx) = mpsc::channel();
        let handle = Some(thread::spawn(move || {
            run(&presence_rx, settings, templates);
        }));
        Self {
            presence_tx,
            handle,
            settings_path,
            thread_seen_dead: false,
            log_reload_handle,
        }
    }

    fn on_timer(&mut self, ctx: &mut Context, _counter: i32) {
        if self.thread_seen_dead {
            return;
        }

        let info = ConnectionInformation::from_ctx(ctx);
        if let Some(info) = info {
            trace!(?info, "Sending controller information to thread.");
            self.send_presence_msg(PresenceMsg::Update(info));
        } else {
            trace!("No controller info to send to thread.");
        }
    }

    fn on_compile_command(&mut self, _ctx: &mut Context, command_line: &str) -> bool {
        if command_line.starts_with(".drp reload") {
            let settings_path = self
                .settings_path
                .clone()
                .map_or_else(Self::make_settings_path, Some);
            if let Some(plugin_path) = &settings_path {
                let plugin_path = plugin_path.display().to_string();
                info!(target: "mbox", "Found settings at `{}`.", plugin_path);
                match Settings::load(&[&plugin_path]) {
                    Ok(settings) => match Templates::new(&settings) {
                        Ok(templates) => {
                            info!(target: "mbox", "Successfully reloaded settings.");
                            reload_log_level(&self.log_reload_handle, &settings.general.log_level);
                            self.send_presence_msg(PresenceMsg::RefreshSettings(Box::new((
                                settings, templates,
                            ))));
                        }
                        Err(err) => {
                            warn!(target: "mbox", %err, "Failed to load templates. Keeping current settings.");
                        }
                    },
                    Err(err) => {
                        warn!(target: "mbox", %err, "Failed to load settings. Keeping current settings.");
                    }
                }
            } else {
                warn!(target: "mbox", "Unable to find settings file, keeping current settings.");
            }

            true
        } else {
            false
        }
    }
}

register_plugin!(DiscordRichPresence);
