use std::time::Instant;

use anyhow::{Result, anyhow};
use rand::rng;
use rand::seq::IndexedRandom;
use serde::Deserialize;
use tera::{Context, Tera};
use tracing::warn;

use crate::{controller_information::ConnectionInformation, settings::Settings};

const DISALLOWED_NAMES: &[&str] = &[
    "connection_type",
    "callsign",
    "frequency",
    "rating",
    "facility",
    "tracked",
    "in_range",
    "radio_name",
    "name",
    "activity_type",
    "status_display_type",
    "details",
    "details_url",
    "state",
    "state_url",
    "assets_large_image",
    "assets_large_text",
    "assets_large_url",
    "assets_small_image",
    "assets_small_text",
    "assets_small_url",
    "buttons_first_label",
    "buttons_first_url",
    "buttons_second_label",
    "buttons_second_url",
];

pub struct Templates {
    tera: Tera,
    extra_templates: Vec<String>,
    idle_tag_lines: Vec<String>,
    idle_tag_line: String,
    idle_tag_line_last_update: Instant,
}

impl Templates {
    pub fn new(settings: &Settings) -> Result<Self> {
        let mut tera = Tera::default();
        let mut extra_templates = Vec::with_capacity(settings.templates.len());

        tera.add_raw_template("name", &settings.activity.name)?;
        tera.add_raw_template("activity_type", &settings.activity.activity_type)?;
        tera.add_raw_template(
            "status_display_type",
            &settings.activity.status_display_type,
        )?;
        tera.add_raw_template("details", &settings.activity.details)?;
        tera.add_raw_template("details_url", &settings.activity.details_url)?;
        tera.add_raw_template("state", &settings.activity.state)?;
        tera.add_raw_template("state_url", &settings.activity.state_url)?;

        tera.add_raw_template("assets_large_image", &settings.activity.assets.large_image)?;
        tera.add_raw_template("assets_large_text", &settings.activity.assets.large_text)?;
        tera.add_raw_template("assets_large_url", &settings.activity.assets.large_url)?;
        tera.add_raw_template("assets_small_image", &settings.activity.assets.small_image)?;
        tera.add_raw_template("assets_small_text", &settings.activity.assets.small_text)?;
        tera.add_raw_template("assets_small_url", &settings.activity.assets.small_url)?;

        tera.add_raw_template(
            "buttons_first_label",
            &settings.activity.buttons.first.label,
        )?;
        tera.add_raw_template("buttons_first_url", &settings.activity.buttons.first.url)?;
        tera.add_raw_template(
            "buttons_second_label",
            &settings.activity.buttons.second.label,
        )?;
        tera.add_raw_template("buttons_second_url", &settings.activity.buttons.second.url)?;

        for extra in &settings.templates {
            if DISALLOWED_NAMES.iter().any(|dn| dn == &extra.name) {
                return Err(anyhow!("Template name {} is not allowed", extra.name));
            }
            extra_templates.push(extra.name.clone());
            tera.add_raw_template(&extra.name, &extra.template)?;
        }

        let idle_tag_lines = {
            let mut lines = Vec::with_capacity(
                settings.idle.tag_lines.len() + settings.idle.extra_tag_lines.len(),
            );
            lines.extend(settings.idle.tag_lines.clone());
            lines.extend(settings.idle.extra_tag_lines.clone());
            lines
        };
        let idle_tag_line = idle_tag_lines
            .choose(&mut rng())
            .map(|s| s.to_owned())
            .unwrap_or_default();

        Ok(Self {
            tera,
            extra_templates,
            idle_tag_lines,
            idle_tag_line,
            idle_tag_line_last_update: Instant::now(),
        })
    }

    pub fn make_context(
        &mut self,
        settings: &Settings,
        info: &ConnectionInformation,
    ) -> Result<Context> {
        if self.idle_tag_line_last_update.elapsed().as_secs()
            >= settings.idle.tag_line_rotate_interval_s
        {
            let idle_tag_line = self
                .idle_tag_lines
                .choose(&mut rng())
                .map(|s| s.to_owned())
                .unwrap_or_default();
            self.idle_tag_line = idle_tag_line;
            self.idle_tag_line_last_update = Instant::now();
        }

        let mut ctx = Context::new();
        ctx.insert("idle_tag_line", &self.idle_tag_line);
        info.enrich_context(&mut ctx, settings);
        for extra in &self.extra_templates {
            let rendered = match self.tera.render(extra, &ctx) {
                Ok(rendered) => rendered,
                Err(err) => {
                    warn!(
                        ?err,
                        template_name = extra,
                        "Failed to render template, falling back to empty string."
                    );
                    String::new()
                }
            };
            ctx.insert(extra.clone(), rendered.trim());
        }
        Ok(ctx)
    }

    pub fn render(&self, template_name: &str, context: &Context) -> Result<String> {
        Ok(self.tera.render(template_name, context)?.trim().to_owned())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
pub enum ActivityType {
    #[default]
    Playing,
    Listening,
    Watching,
    Competing,
}

impl From<ActivityType> for discord_rich_presence::activity::ActivityType {
    fn from(val: ActivityType) -> Self {
        match val {
            ActivityType::Playing => Self::Playing,
            ActivityType::Listening => Self::Listening,
            ActivityType::Watching => Self::Watching,
            ActivityType::Competing => Self::Competing,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq)]
pub enum StatusDisplayType {
    #[default]
    Name,
    State,
    Details,
}

impl From<StatusDisplayType> for discord_rich_presence::activity::StatusDisplayType {
    fn from(val: StatusDisplayType) -> Self {
        match val {
            StatusDisplayType::Name => Self::Name,
            StatusDisplayType::State => Self::State,
            StatusDisplayType::Details => Self::Details,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Templates;
    use crate::{controller_information::ConnectionInformation, settings::Settings};

    #[test]
    fn default_templates_load() {
        let settings = Settings::load(&[]).expect("settings");
        Templates::new(&settings).expect("templates");
    }

    #[test]
    fn make_context_idle() {
        let settings = Settings::load(&[]).expect("settings");
        let mut templates = Templates::new(&settings).expect("templates");

        let info = ConnectionInformation::Idle;

        templates.make_context(&settings, &info).expect("context");
    }
}
