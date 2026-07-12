//! EuroScope plugin that publishes the controller's active position to Discord
//! Rich Presence.
//!
//! The plugin reads the logged-in controller ("myself") once per second in
//! [`Plugin::on_timer`] and, whenever it changes, hands the snapshot to a
//! background [`Presence`] worker. Nothing here ever blocks EuroScope's main
//! thread — all Discord IPC happens on the worker thread.

mod presence;

use euroscope::{Context, Plugin, register_plugin};

use crate::presence::{MyPosition, Presence, Session};

struct DiscordRichPresence {
    presence: Presence,
    /// Last session pushed to the worker; used to avoid redundant updates.
    current: Option<Session>,
}

#[expect(
    clippy::missing_trait_methods,
    reason = "We don't need all the callbacks"
)]
impl Plugin for DiscordRichPresence {
    const AUTHOR: &'static str = "Marc Schmitt <vatsim@risson.space>";
    const COPYRIGHT: &'static str = "MIT OR Apache-2.0";
    const NAME: &'static str = "Discord RPC";
    const VERSION: &'static str = env!("CARGO_PKG_VERSION");

    fn new(ctx: &mut Context) -> Self {
        ctx.display_message(
            Self::NAME,
            "",
            &format!("{} v{} loaded", Self::NAME, Self::VERSION),
        );
        Self {
            presence: Presence::start(),
            current: None,
        }
    }

    fn on_timer(&mut self, ctx: &mut Context, _counter: i32) {
        let session = MyPosition::current(ctx).map(|position| Session {
            tracked: ctx.aircraft_tracked_by_me(),
            in_range: ctx.aircraft_in_range(),
            position,
        });
        if session != self.current {
            self.current.clone_from(&session);
            self.presence.update(session);
        }
    }
}

register_plugin!(DiscordRichPresence);
