use euroscope::{
    get_plugin_path,
    tracing::{MboxLayer, is_mbox_target},
};
use tracing::{Level, error};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, filter::filter_fn, fmt, prelude::*};

use crate::settings::Settings;

pub fn install(settings: &Settings) {
    let filter_layer = EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .parse_lossy(&settings.general.log_level);

    if let Some(plugin_path) = get_plugin_path()
        && let Some(plugin_dir) = plugin_path.parent()
    {
        let logs_dir = plugin_dir.join("logs");
        if let Ok(file_appender) = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_suffix("log")
            .build(logs_dir)
        {
            tracing_subscriber::registry()
                .with(MboxLayer::new().with_filter(filter_fn(is_mbox_target)))
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(file_appender)
                        .with_filter(filter_layer),
                )
                .init();
            return;
        }
    }

    error!(
        "Could not log to file, falling back to writing logs to EuroScope message box. This is a \
         bug, please report it."
    );
    tracing_subscriber::registry()
        .with(MboxLayer::new().with_filter(filter_layer))
        .init();
}

/// Install a very basic tracing subscriber until a fully-featured one can be installed.
///
/// Sends all data to the EuroScope message box.
#[must_use]
pub fn install_crude() -> tracing::dispatcher::DefaultGuard {
    let subscriber = tracing_subscriber::registry().with(MboxLayer::new());

    tracing::dispatcher::set_default(&subscriber.into())
}
