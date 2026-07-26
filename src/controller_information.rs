use euroscope::{ConnectionType, Context, ControllerRating, Facility};

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

#[derive(Debug, Default, Clone, PartialEq)]
pub enum ConnectionInformation {
    #[default]
    Idle,
    Connected(ControllerInformation),
    Sweatbox(ControllerInformation),
    Playback(ControllerInformation),
}

impl ConnectionInformation {
    pub fn from_ctx(ctx: &Context) -> Option<Self> {
        match ctx.connection_type() {
            ConnectionType::Direct => Some(
                ControllerInformation::from_ctx(ctx)
                    .map(Self::Connected)
                    .unwrap_or_default(),
            ),
            ConnectionType::Sweatbox => Some(
                ControllerInformation::from_ctx(ctx)
                    .map(Self::Sweatbox)
                    .unwrap_or_default(),
            ),
            ConnectionType::Playback => Some(
                ControllerInformation::from_ctx(ctx)
                    .map(Self::Playback)
                    .unwrap_or_default(),
            ),
            ConnectionType::None => Some(Self::Idle),
            _ => None,
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
                if (info.frequency - 199.998_f64).abs() > 0.001_f64 {
                    ctx.insert("frequency", &format!("{:.3}", info.frequency));
                }
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

#[cfg(test)]
mod tests {
    use euroscope::{ControllerRating, Facility};

    use crate::{
        controller_information::{ConnectionInformation, ControllerInformation},
        settings::Settings,
    };

    fn empty_controller_info() -> ControllerInformation {
        ControllerInformation {
            callsign: String::new(),
            frequency: 0.0_f64,
            rating: ControllerRating::Unknown,
            facility: Facility::Observer,
            tracked: 0,
            in_range: 0,
        }
    }

    #[test]
    fn others_as_direct() {
        let mut settings = Settings::load(&[]).expect("settings");
        settings.general.treat_other_connections_as_direct = true;

        let info = ConnectionInformation::Sweatbox(empty_controller_info());
        assert_eq!(info.label(&settings), "Connected");

        let info = ConnectionInformation::Playback(empty_controller_info());
        assert_eq!(info.label(&settings), "Connected");
    }
}
