use euroscope::{ConnectionType, Context, ControllerRating, Facility};
use serde::Deserialize;

use crate::settings::Settings;

#[derive(Debug, Clone, PartialEq)]
pub struct ControllerInformation {
    pub callsign: String,
    pub frequency: f64,
    pub rating: ControllerRating,
    pub facility: Facility,
    pub tracked: usize,
    pub in_range: usize,
}

impl ControllerInformation {
    fn from_ctx(ctx: &Context) -> Option<Self> {
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
            frequency: me.primary_frequency(),
            rating: me.rating(),
            facility: me.facility(),
            tracked: ctx
                .flight_plans()
                .filter(|fp| fp.tracking_controller_is_me())
                .count(),
            in_range: ctx.radar_targets().count(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum ConnectionInformation {
    #[default]
    Idle,
    Connected(ControllerInformation),
    Sweatbox(ControllerInformation),
    Playback(ControllerInformation),
}

impl ConnectionInformation {
    pub fn from_ctx(ctx: &Context) -> Self {
        match ctx.connection_type() {
            ConnectionType::Direct => ControllerInformation::from_ctx(ctx)
                .map(Self::Connected)
                .unwrap_or_default(),
            ConnectionType::Sweatbox => ControllerInformation::from_ctx(ctx)
                .map(Self::Sweatbox)
                .unwrap_or_default(),
            ConnectionType::Playback => ControllerInformation::from_ctx(ctx)
                .map(Self::Playback)
                .unwrap_or_default(),
            _ => Self::Idle,
        }
    }

    pub fn label(&self, settings: &Settings) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Connected(_) => "Connected",
            Self::Sweatbox(_) => {
                if settings.general.treat_other_connections_as_direct {
                    "Connected"
                } else {
                    "Sweatbox"
                }
            }
            Self::Playback(_) => {
                if settings.general.treat_other_connections_as_direct {
                    "Connected"
                } else {
                    "Playback"
                }
            }
        }
    }

    pub fn enrich_context(&self, ctx: &mut tera::Context, settings: &Settings) {
        ctx.insert("connection_type", self.label(settings));
        match self {
            Self::Connected(info) | Self::Playback(info) | Self::Sweatbox(info) => {
                ctx.insert("callsign", &info.callsign);
                ctx.insert("frequency", &format!("{:.3}", info.frequency));
                ctx.insert("rating", info.rating.label());
                // TODO: add a .label() in euroscope crate
                let facility = match info.facility {
                    Facility::Observer => "Observer",
                    Facility::FlightService => "Flight Service",
                    Facility::Delivery => "Delivery",
                    Facility::Ground => "Ground",
                    Facility::Tower => "Tower",
                    Facility::Approach => "Approach",
                    Facility::Center => "Center",
                    Facility::Other(_) => "",
                };
                ctx.insert("facility", facility);
                ctx.insert("tracked", &info.tracked);
                ctx.insert("in_range", &info.in_range);
            }
            Self::Idle => {}
        }
    }
}
