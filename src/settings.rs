use std::collections::HashMap;

use anyhow::Result;
use config::{Config, File, FileFormat};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = include_str!("../default.toml");

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct Settings {
    general: GeneralSettings,
    discord: DiscordSettings,
    radio_names: HashMap<String, String>,
    activity: ActivitySettings,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct GeneralSettings {
    activity_retry_interval_s: u64,
    activity_min_push_interval_s: u64,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct DiscordSettings {
    client_id: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ActivitySettings {
    name: String,
    activity_type: ActivityType,
    status_display_type: StatusDisplayType,
    details: String,
    details_url: String,
    state: String,
    state_url: String,
    assets: ActivityAssetsSettings,
    buttons: ActivityButtonsSettings,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) enum ActivityType {
    Playing,
    Listening,
    Watching,
    Competing,
}

impl Into<discord_rich_presence::activity::ActivityType> for ActivityType {
    fn into(self) -> discord_rich_presence::activity::ActivityType {
        match self {
            Self::Playing => discord_rich_presence::activity::ActivityType::Playing,
            Self::Listening => discord_rich_presence::activity::ActivityType::Listening,
            Self::Watching => discord_rich_presence::activity::ActivityType::Watching,
            Self::Competing => discord_rich_presence::activity::ActivityType::Competing,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) enum StatusDisplayType {
    Name,
    State,
    Details,
}

impl Into<discord_rich_presence::activity::StatusDisplayType> for StatusDisplayType {
    fn into(self) -> discord_rich_presence::activity::StatusDisplayType {
        match self {
            Self::Name => discord_rich_presence::activity::StatusDisplayType::Name,
            Self::State => discord_rich_presence::activity::StatusDisplayType::State,
            Self::Details => discord_rich_presence::activity::StatusDisplayType::Details,
        }
    }
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ActivityAssetsSettings {
    large_image: String,
    large_text: String,
    large_url: String,
    small_image: String,
    small_text: String,
    small_url: String,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ActivityButtonsSettings {
    first: ActivityButtonSettings,
    second: ActivityButtonSettings,
}

#[derive(Debug, Deserialize, PartialEq)]
pub(crate) struct ActivityButtonSettings {
    #[serde(default)]
    label: String,
    #[serde(default)]
    url: String,
}

impl Settings {
    pub(crate) fn load() -> Result<Self> {
        let raw = Config::builder()
            .add_source(File::from_str(DEFAULT_CONFIG, FileFormat::Toml))
            .build()?;
        Ok(raw.try_deserialize()?)
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn default_config_loads() {
        Settings::load().unwrap();
    }
}
