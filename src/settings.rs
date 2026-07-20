use std::collections::HashMap;

use anyhow::Result;
use config::{Config, File, FileFormat};
use serde::Deserialize;

const DEFAULT_CONFIG: &str = include_str!("../default.toml");

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct Settings {
    pub general: GeneralSettings,
    pub discord: DiscordSettings,
    pub activity: ActivitySettings,
    pub radio_names: HashMap<String, String>,
    pub idle: IdleSettings,
    pub templates: Vec<TemplateSettings>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct GeneralSettings {
    pub log_level: String,
    pub activity_retry_interval_s: u64,
    pub activity_min_push_interval_s: u64,
    pub treat_other_connections_as_direct: bool,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DiscordSettings {
    pub client_id: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ActivitySettings {
    pub name: String,
    pub activity_type: String,
    pub status_display_type: String,
    pub details: String,
    pub details_url: String,
    pub state: String,
    pub state_url: String,
    pub assets: ActivityAssetsSettings,
    pub buttons: ActivityButtonsSettings,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ActivityAssetsSettings {
    pub large_image: String,
    pub large_text: String,
    pub large_url: String,
    pub small_image: String,
    pub small_text: String,
    pub small_url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ActivityButtonsSettings {
    pub first: ActivityButtonSettings,
    pub second: ActivityButtonSettings,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ActivityButtonSettings {
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct IdleSettings {
    pub set_presence_when_idle: bool,
    pub tag_line_rotate_interval_s: u64,
    pub tag_lines: Vec<String>,
    pub extra_tag_lines: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct TemplateSettings {
    pub name: String,
    pub template: String,
}

impl Settings {
    pub fn load(extra_paths: &[&str]) -> Result<Self> {
        let mut builder =
            Config::builder().add_source(File::from_str(DEFAULT_CONFIG, FileFormat::Toml));
        for path in extra_paths {
            builder = builder.add_source(File::new(path, FileFormat::Toml));
        }
        let raw = builder.build()?;
        Ok(raw.try_deserialize()?)
    }
}

#[cfg(test)]
mod tests {
    use super::Settings;

    #[test]
    fn default_config_loads() {
        Settings::load(&[]).expect("settings");
    }
}
