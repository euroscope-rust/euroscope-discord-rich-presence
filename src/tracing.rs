use euroscope::{
    get_plugin_path,
    tracing::{MboxLayer, is_mbox_target},
};
use tracing::{Level, error};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, Registry, filter::filter_fn, fmt, prelude::*, reload};

use crate::settings::Settings;

pub type LogReloadHandle = reload::Handle<EnvFilter, Registry>;

fn make_env_filter(log_level: &str) -> EnvFilter {
    EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .parse_lossy(log_level)
}

pub fn reload_log_level(handle: &LogReloadHandle, log_level: &str) {
    let filter_layer = make_env_filter(log_level);
    if let Err(err) = handle.reload(filter_layer) {
        error!(%err, "Failed to reload log level.");
    }
}

pub fn install(settings: &Settings) -> LogReloadHandle {
    let filter_layer = make_env_filter(&settings.general.log_level);
    let (filter_layer, reload_handle) = reload::Layer::new(filter_layer);

    if let Some(plugin_path) = get_plugin_path() {
        let logs_dir = plugin_path.with_extension("logs");
        if let Ok(file_appender) = RollingFileAppender::builder()
            .rotation(Rotation::DAILY)
            .filename_suffix("log")
            .build(logs_dir)
        {
            tracing_subscriber::registry()
                .with(
                    fmt::layer()
                        .json()
                        .with_writer(file_appender)
                        .with_filter(filter_layer),
                )
                .with(MboxLayer::new().with_filter(filter_fn(is_mbox_target)))
                .init();
            return reload_handle;
        }
    }

    error!(
        "Could not log to file, falling back to writing logs to EuroScope message box. This is a \
         bug, please report it."
    );
    tracing_subscriber::registry()
        .with(MboxLayer::new().with_filter(filter_layer))
        .init();

    reload_handle
}

/// Install a very basic tracing subscriber until a fully-featured one can be installed.
///
/// Sends all data to the EuroScope message box.
#[must_use]
pub fn install_crude() -> tracing::dispatcher::DefaultGuard {
    let subscriber = tracing_subscriber::registry().with(MboxLayer::new());

    tracing::dispatcher::set_default(&subscriber.into())
}
