use anyhow::Result;
use serde::Deserialize;
use tera::{Context, Tera};

use crate::{controller_information::ConnectionInformation, settings::Settings};

pub struct Templates {
    tera: Tera,
    extra_templates: Vec<String>,
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
            extra_templates.push(extra.name.clone());
            tera.add_raw_template(&extra.name, &extra.template)?;
        }

        Ok(Self {
            tera,
            extra_templates,
        })
    }

    pub fn make_context(
        &self,
        settings: &Settings,
        info: &ConnectionInformation,
    ) -> Result<Context> {
        let mut ctx = Context::new();
        info.enrich_context(&mut ctx, settings);
        for extra in &self.extra_templates {
            let rendered = self.tera.render(extra, &ctx)?;
            ctx.insert(extra.clone(), &rendered);
        }
        Ok(ctx)
    }

    pub fn render(&self, template_name: &str, context: &Context) -> Result<String> {
        Ok(self.tera.render(template_name, context)?.trim().to_owned())
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
pub enum ActivityType {
    #[default]
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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq)]
pub enum StatusDisplayType {
    #[default]
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

#[cfg(test)]
mod tests {
    use crate::settings::Settings;

    use super::Templates;

    #[test]
    fn default_templates_load() {
        let settings = Settings::load(&[]).expect("Failed to load default settings.");
        Templates::new(&settings).expect("Failed to load default templates.");
    }
}
