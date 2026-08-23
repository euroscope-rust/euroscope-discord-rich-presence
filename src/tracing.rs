use std::sync::OnceLock;

use euroscope::{
    get_plugin_path,
    tracing::{MboxLayer, is_mbox_target},
};
use tracing::{Level, error, warn};
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{
    EnvFilter, Registry, filter::filter_fn, fmt, prelude::*, reload,
};

use crate::settings::Settings;

type LogFilterHandle = reload::Handle<EnvFilter, Registry>;

static LOG_FILTER_RELOAD: OnceLock<LogFilterHandle> = OnceLock::new();

fn build_env_filter(log_level: &str) -> EnvFilter {
    EnvFilter::builder()
        .with_default_directive(Level::INFO.into())
        .parse_lossy(log_level)
}

/// Install the fully-featured tracing subscriber.
pub fn install(settings: &Settings) {
    let filter_layer = build_env_filter(&settings.general.log_level);
    let (filter_layer, reload_handle) = reload::Layer::new(filter_layer);

    if let Some(plugin_path) = get_plugin_path() {
        let logs_dir = plugin_path.with_extension("logs");
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
            let _ = LOG_FILTER_RELOAD.set(reload_handle);
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
    let _ = LOG_FILTER_RELOAD.set(reload_handle);
}

/// Reload the log filter from a new `log_level` EnvFilter directive string.
pub fn reload_log_level(log_level: &str) {
    let Some(handle) = LOG_FILTER_RELOAD.get() else {
        warn!(target: "mbox", "Log filter reload handle is not available; log_level unchanged.");
        return;
    };
    let new_filter = build_env_filter(log_level);
    if let Err(err) = handle.reload(new_filter) {
        warn!(target: "mbox", %err, "Failed to reload log_level filter.");
    } else {
        tracing::info!(target: "mbox", %log_level, "Reloaded log_level filter.");
    }
}

/// Install a very basic tracing subscriber until a fully-featured one can be installed.
///
/// Sends all data to the EuroScope message box.
#[must_use]
pub fn install_crude() -> tracing::dispatcher::DefaultGuard {
    let subscriber = tracing_subscriber::registry().with(MboxLayer::new());

    tracing::dispatcher::set_default(&subscriber.into())
}