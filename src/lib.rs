//! EuroScope plugin that publishes the controller's active position to Discord
//! Rich Presence.
//!
//! The plugin reads the logged-in controller ("myself") once per second in
//! [`Plugin::on_timer`] and, whenever it changes, hands the snapshot to a
//! background [`Presence`] worker. Nothing here ever blocks EuroScope's main
//! thread — all Discord IPC happens on the worker thread.

pub mod controller_information;
pub mod presence;
pub mod settings;
pub mod template;
pub mod utils;

use std::{fs::metadata, path::PathBuf, sync::mpsc};

use euroscope::{Context, Plugin, register_plugin};

use crate::{
    controller_information::ConnectionInformation, presence::Presence, settings::Settings,
    template::Templates, utils::get_plugin_path,
};

pub enum MainMsg {
    Log(String),
}

struct DiscordRichPresence {
    presence: Presence,
    settings_path: Option<PathBuf>,
    thread_seen_dead: bool,
    main_rx: mpsc::Receiver<MainMsg>,
}

impl DiscordRichPresence {
    fn make_settings_path() -> Option<PathBuf> {
        if let Some(mut path) = get_plugin_path() {
            path.set_extension("toml");
            if metadata(&path).is_ok_and(|m| m.is_file()) {
                Some(path)
            } else {
                None
            }
        } else {
            None
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

    fn new(ctx: &mut Context) -> Self {
        ctx.display_message(
            Self::NAME,
            "",
            &format!("{} v{} loaded", Self::NAME, Self::VERSION),
        );

        let settings_path = Self::make_settings_path();

        let mut settings = if let Some(plugin_path) = &settings_path {
            let plugin_path = plugin_path.display().to_string();
            ctx.display_message(Self::NAME, "", &format!("Found settings at {plugin_path}"));
            Settings::load(&[&plugin_path]).unwrap_or_else(|err| {
                ctx.display_message(
                    Self::NAME,
                    "",
                    &format!("Unable to load settings. Running with default settings."),
                );
                ctx.display_message(Self::NAME, "", &err.to_string());
                Settings::load(&[]).expect("Failed to load default settings.")
            })
        } else {
            ctx.display_message(
                Self::NAME,
                "",
                "Unable to find plugin path. Running with default settings.",
            );
            Settings::load(&[]).expect("Failed to load default settings.")
        };

        let templates = match Templates::new(&settings) {
            Ok(templates) => templates,
            Err(err) => {
                ctx.display_message(
                    Self::NAME,
                    "",
                    "Failed to load templates. Falling back to default settings.",
                );
                settings = Settings::load(&[]).expect("Failed to load default settings.");
                ctx.display_message(Self::NAME, "", &err.to_string());
                Templates::new(&settings).expect("Failed to load default templates.")
            }
        };

        let info = ConnectionInformation::from_ctx(ctx);

        let (main_tx, main_rx) = mpsc::channel();

        let presence = Presence::start(main_tx, settings, templates, info);
        Self {
            settings_path,
            presence,
            thread_seen_dead: false,
            main_rx,
        }
    }

    fn on_timer(&mut self, ctx: &mut Context, _counter: i32) {
        if !self.thread_seen_dead && self.presence.is_thread_dead() {
            self.thread_seen_dead = true;
            ctx.display_message(
                Self::NAME,
                "",
                "Discord presence update thread has died. This is a bug.",
            );
        }
        if !self.thread_seen_dead {
            let info = ConnectionInformation::from_ctx(ctx);
            if !self.presence.send_update(info) {
                ctx.display_message(
                    Self::NAME,
                    "",
                    "Discord presence update thread has died. This is a bug.",
                );
                self.thread_seen_dead = true;
            }
        }
    }

    fn on_compile_command(&mut self, ctx: &mut Context, command_line: &str) -> bool {
        if command_line.starts_with(".drp refresh") {
            let settings_path = self
                .settings_path
                .clone()
                .map(Some)
                .unwrap_or_else(|| Self::make_settings_path());
            if let Some(plugin_path) = &settings_path {
                let plugin_path = plugin_path.display().to_string();
                ctx.display_message(Self::NAME, "", &format!("Found settings at {plugin_path}"));
                match Settings::load(&[&plugin_path]) {
                    Ok(settings) => match Templates::new(&settings) {
                        Ok(templates) => {
                            ctx.display_message(
                                Self::NAME,
                                "",
                                &format!("Successfully reloaded settings."),
                            );
                            if !self.presence.refresh_settings(settings, templates) {
                                ctx.display_message(
                                    Self::NAME,
                                    "",
                                    "Discord presence update thread has died. This is a bug.",
                                );
                                self.thread_seen_dead = true;
                            }
                        }
                        Err(err) => {
                            ctx.display_message(
                                Self::NAME,
                                "",
                                &format!("Failed to load templates. Keeping current settings."),
                            );
                            ctx.display_message(Self::NAME, "", &err.to_string());
                        }
                    },
                    Err(err) => {
                        ctx.display_message(
                            Self::NAME,
                            "",
                            &format!("Unable to load settings. Keeping current settings."),
                        );
                        ctx.display_message(Self::NAME, "", &err.to_string());
                    }
                };
            } else {
                ctx.display_message(
                    Self::NAME,
                    "",
                    "Unable to find plugin path. Keeping current settings.",
                );
            };

            true
        } else {
            false
        }
    }
}

register_plugin!(DiscordRichPresence);
