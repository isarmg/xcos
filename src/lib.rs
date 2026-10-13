mod auth;
mod background;
mod camera_state;
mod config;
mod crypto;
mod doctor;
mod error;
mod history;
mod lifecycle;
mod mediamtx;
mod models;
mod protocol;
mod reconciliation;
mod routes;
mod runtime_lock;
mod sqlite;
mod static_assets;

mod app;
pub mod offline;

pub use app::AppState;
pub(crate) use xcss::server_cli::CliError as CliFailure;

pub async fn run(root: &std::path::Path) -> anyhow::Result<()> {
    use tracing_subscriber::prelude::*;
    let loaded = config::load_at_root(root, &root.join("config/config.json"))?;
    let log_layer = xcss::log::XcssStructuredLayer::new("xcos")?;
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(&loaded.value.log_level))
        .with(log_layer.clone())
        .init();
    let effective = loaded.value.effective()?;
    let (media_config, media_contract, media_binary) = loaded.value.companion_paths()?;
    let recordings = loaded
        .value
        .recordings_directory
        .clone()
        .unwrap_or(loaded.value.data_directory()?.join("recordings"));
    offline::verify_media_environment(root)?;
    doctor::verify_companion(
        &media_contract,
        &media_binary,
        &media_config,
        &recordings,
        effective.media_callback_bind,
    )
    .await?;
    app::serve(effective, &log_layer).await
}
