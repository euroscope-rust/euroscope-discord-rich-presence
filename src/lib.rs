#![doc = include_str!("../README.md")]

pub mod commands;
pub mod controller_information;
pub mod presence;
pub mod settings;
pub mod status;
pub mod templates;
pub mod tracing;
pub mod utils;

use std::{
    fs::metadata,
    path::PathBuf,
    sync::{Arc, mpsc},
    thread,
    thread::JoinHandle,
};

use ::tracing::{Span, error, error_span, info, span::EnteredSpan, trace, warn};
use euroscope::{Context, Plugin, get_plugin_path, register_plugin};
use uuid::Uuid;

use crate::{
    commands::{Command, HELP, HELP_HANDLER},
    controller_information::ConnectionInformation,
    presence::{PresenceMsg, run},
    settings::Settings,
    status::SharedStatus,
    templates::Templates,
    tracing::{LogReloadHandle, reload_log_level},
};

struct DiscordRichPresence {
    presence_tx: mpsc::Sender<PresenceMsg>,
    handle: Option<JoinHandle<()>>,
    settings_path: Option<PathBuf>,
    thread_seen_dead: bool,
    /// Whether we're publishing to Discord, toggled by `.drp start` / `.drp
    /// stop`. Held here as well as in the worker so a stopped plugin doesn't
    /// keep feeding the worker updates it would only throw away.
    processing: bool,
    status: Arc<SharedStatus>,
    log_reload_handle: LogReloadHandle,
    _instance_span: EnteredSpan,
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

    /// Reload the settings file, keeping the current settings if the new ones
    /// don't load.
    fn reload_settings(&mut self) {
        let settings_path = self
            .settings_path
            .clone()
            .map_or_else(Self::make_settings_path, Some);
        let Some(plugin_path) = &settings_path else {
            warn!(target: "mbox", "Unable to find settings file, keeping current settings.");
            return;
        };

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
    }

    /// Start or stop publishing to Discord, telling the user either way.
    fn set_processing(&mut self, ctx: &Context, processing: bool) {
        if self.processing == processing {
            Self::say(
                ctx,
                if processing {
                    "Already running."
                } else {
                    "Already stopped."
                },
            );
            return;
        }

        self.processing = processing;
        self.send_presence_msg(PresenceMsg::SetProcessing(processing));
        if processing {
            // Hand the worker the state as it is now: the last one it saw
            // dates from before we stopped, and the next timer tick is only
            // ever a tick away, but there's no reason to publish something
            // stale in the meantime.
            self.send_presence_msg(PresenceMsg::Update(ConnectionInformation::from_ctx(ctx)));
        }

        info!(processing, "Processing toggled by command.");
        Self::say(
            ctx,
            if processing {
                "Started: updates will be sent to Discord again."
            } else {
                "Stopped: no more updates will be sent, and Discord will be disconnected."
            },
        );
    }

    /// Report what the plugin is doing, from our own state and the worker's
    /// latest snapshot.
    fn show_status(&self, ctx: &Context) {
        Self::say(ctx, &format!("{} {} status:", Self::NAME, Self::VERSION));
        Self::say(
            ctx,
            &format!(
                "  Processing: {}",
                if self.processing {
                    "running"
                } else {
                    "stopped"
                }
            ),
        );
        Self::say(
            ctx,
            &format!(
                "  Settings file: {}",
                self.settings_path.as_ref().map_or_else(
                    || "none, running with defaults".to_owned(),
                    |path| path.display().to_string()
                )
            ),
        );
        if self.thread_seen_dead {
            Self::say(
                ctx,
                "  Worker thread: dead. This is a bug, please report it.",
            );
        }
        for line in self.status.get().report() {
            Self::say(ctx, &line);
        }
    }

    /// List the available commands.
    ///
    /// Written to the message box directly rather than through the `tracing`
    /// integration: this is a reply to the user, not a log line, and going
    /// through `tracing` would also hold it back until the callback returns,
    /// putting it out of order with the rest of a command's output.
    fn show_help(ctx: &Context, handler: &str, sender: &str) {
        ctx.display_message(
            handler,
            sender,
            &format!("{} {} - available commands:", Self::NAME, Self::VERSION),
        );
        for (command, description) in HELP {
            ctx.display_message(handler, sender, &format!("  {command:<12} - {description}"));
        }
    }

    /// Say something to the user under our own handler, as themselves.
    fn say(ctx: &Context, message: &str) {
        ctx.display_message(Self::NAME, "", message);
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
        let instance_id = Uuid::new_v4();
        let instance_span = error_span!("root", %instance_id).entered();
        info!(
            target: "mbox",
            %instance_id,
            "Log statements from this EuroScope window are tagged with this instance id."
        );
        let presence_span = Span::clone(&instance_span);

        let (presence_tx, presence_rx) = mpsc::channel();
        let status = Arc::new(SharedStatus::default());
        let presence_status = Arc::clone(&status);
        let handle = Some(thread::spawn(move || {
            presence_span.in_scope(|| run(&presence_rx, &presence_status, settings, templates));
        }));
        Self {
            presence_tx,
            handle,
            settings_path,
            thread_seen_dead: false,
            processing: true,
            status,
            log_reload_handle,
            _instance_span: instance_span,
        }
    }

    fn on_timer(&mut self, ctx: &mut Context, _counter: i32) {
        if self.thread_seen_dead || !self.processing {
            return;
        }

        let info = ConnectionInformation::from_ctx(ctx);
        trace!(?info, "Sending controller information to thread.");
        self.send_presence_msg(PresenceMsg::Update(info));
    }

    fn on_compile_command(&mut self, ctx: &mut Context, command_line: &str) -> bool {
        let Some(command) = Command::parse(command_line) else {
            return false;
        };

        match command {
            Command::Reload => self.reload_settings(),
            Command::Start => self.set_processing(ctx, true),
            Command::Stop => self.set_processing(ctx, false),
            Command::Status => self.show_status(ctx),
            Command::Help => Self::show_help(ctx, Self::NAME, ""),
            Command::HelpDrp => Self::show_help(ctx, HELP_HANDLER, Self::NAME),
            Command::HelpIndex => {
                // `.help` belongs to every plugin at once: announce ourselves
                // and hand it on rather than claiming it.
                ctx.display_message(
                    HELP_HANDLER,
                    HELP_HANDLER,
                    &format!(".HELP DRP | {} Help", Self::NAME),
                );
                return false;
            }
            Command::Unknown => {
                Self::say(ctx, &format!("Unknown command: `{command_line}`."));
                Self::show_help(ctx, Self::NAME, "");
            }
        }

        true
    }
}

register_plugin!(DiscordRichPresence);
