use crate::{app, camera_state, config, doctor, release, sqlite, static_assets, CliFailure};
use clap::{Parser, Subcommand};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "xcos",
    version,
    long_version = concat!(env!("CARGO_PKG_VERSION"), " target=", env!("XCOS_BUILD_TARGET"), " source=", env!("XCOS_SOURCE_REVISION")),
    about = "Xcos camera monitoring control plane"
)]
struct Cli {
    #[arg(long, global = true, env = "XCOS_CONFIG", hide_env_values = true)]
    config: Option<PathBuf>,
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Initialize a new state with an administrator password read from stdin.
    Init {
        #[arg(long, default_value = "admin")]
        username: String,
        #[command(flatten)]
        settings: config::Overrides,
    },
    /// Run the current initialized data and embedded Web resources.
    Run {
        #[command(flatten)]
        settings: config::Overrides,
    },
    /// Validate configuration without requiring initialized state.
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
    /// Query actual readiness with exact service identity verification.
    Status {
        #[command(flatten)]
        settings: config::Overrides,
    },
    /// Check initialized state and companion contracts offline.
    Check {
        #[command(flatten)]
        settings: config::Overrides,
    },
    /// Offline administrator maintenance.
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    /// Render the effective MediaMTX configuration to stdout.
    MediaConfig {
        #[command(flatten)]
        settings: config::Overrides,
    },
    /// Print the static asset contract embedded in this build.
    #[command(hide = true)]
    StaticContract,
    /// Print the complete xcss-generated Web inventory.
    #[command(hide = true)]
    WebAssets,
    /// Print the complete identity compiled into this binary.
    ReleaseIdentity,
    StateContract,
    /// Verify a complete immutable release with its own binary.
    VerifyRelease {
        release_root: PathBuf,
    },
    /// Print the canonical release-manifest identity header.
    #[command(hide = true)]
    ReleaseManifestHeader,
}

#[derive(Subcommand)]
enum ConfigCommand {
    Validate {
        #[command(flatten)]
        settings: config::Overrides,
    },
}

#[derive(Subcommand)]
enum AdminCommand {
    ResetPassword {
        #[arg(long, default_value = "admin")]
        username: String,
        #[command(flatten)]
        settings: config::Overrides,
    },
}

pub(crate) async fn run() -> std::process::ExitCode {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    let json = arguments.iter().any(|argument| argument == "--json");
    let mut cli = match Cli::try_parse_from(arguments) {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
            ) =>
        {
            let _ = error.print();
            return std::process::ExitCode::SUCCESS;
        }
        Err(_) => {
            return xcss::server_cli::report_error(
                &xcss::server_cli::ErrorEnvelope::new(
                    xcss::server_cli::HttpStatus::BadRequest,
                    "Command arguments do not satisfy the current contract.",
                ),
                json,
                2,
            );
        }
    };
    if let Some(path) = cli.config.take() {
        let current = if path
            .components()
            .any(|part| matches!(part, std::path::Component::ParentDir))
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "unsafe configuration path",
            ))
        } else {
            std::path::absolute(path)
        };
        match current {
            Ok(path) => cli.config = Some(path),
            Err(_) => {
                return xcss::server_cli::report_error(
                    &xcss::config::ConfigError::new(
                        xcss::config::Reason::InvalidValue,
                        "/config",
                        xcss::config::ConfigSource::CommandLine,
                    )
                    .envelope(),
                    json,
                    2,
                )
            }
        }
    }
    use tracing_subscriber::prelude::*;
    let log_level = config::load(
        cli.config.as_deref(),
        cli.data_dir.as_deref(),
        &config::Overrides::default(),
    )
    .map(|loaded| loaded.value.log_level)
    .unwrap_or_else(|_| "info".into());
    let log_layer = xcss::log::XcssStructuredLayer::new("xcos").expect("static service identity");
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(log_level))
        .with(log_layer.clone())
        .init();
    match execute(cli, &log_layer).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            let envelope = if let Some(error) = error.downcast_ref::<xcss::config::ConfigError>() {
                error.envelope()
            } else if let Some(error) = error.downcast_ref::<xcss::state_file::Error>() {
                xcss::server_cli::state_error(error)
            } else if let Some(error) = error.downcast_ref::<xcss::server_cli::SnapshotError>() {
                xcss::server_cli::snapshot_error(error)
            } else if let Some(auth_error) = error.downcast_ref::<xcss::admin_auth::Error>() {
                let path = match auth_error {
                    xcss::admin_auth::Error::InvalidAdministratorUsername => Some("/username"),
                    xcss::admin_auth::Error::InvalidPassword => Some("/password"),
                    xcss::admin_auth::Error::InvalidPasswordHash => Some("/password_hash"),
                    _ => None,
                };
                if let Some(path) = path {
                    xcss::server_cli::ErrorEnvelope::with_code(
                        xcss::server_cli::ErrorCode::new("auth.invalid_request")
                            .expect("static code"),
                        "Administrator credentials do not satisfy the current contract.",
                    )
                    .with_detail("reason", "INVALID_VALUE")
                    .with_detail("path", path)
                } else {
                    xcss::server_cli::ErrorEnvelope::with_code(
                        xcss::server_cli::ErrorCode::new("auth.operation_failed")
                            .expect("static code"),
                        "The administrator operation could not be completed.",
                    )
                }
            } else if let Some(error) = error.downcast_ref::<CliFailure>() {
                error.0.clone()
            } else if error
                .downcast_ref::<xcss::schema_identity::Error>()
                .is_some()
                || matches!(
                    error.downcast_ref::<xcss::sqlite::Error>(),
                    Some(
                        xcss::sqlite::Error::SchemaBudgetExceeded
                            | xcss::sqlite::Error::ProductMetadataTableMissing
                            | xcss::sqlite::Error::ProductMetadataStorageClass { .. }
                            | xcss::sqlite::Error::SchemaIdentity(_)
                    )
                )
            {
                xcss::server_cli::ErrorEnvelope::with_code(
                    xcss::server_cli::ErrorCode::new("contract_violation").expect("static code"),
                    "The actual database structure does not satisfy the current schema contract.",
                )
            } else {
                tracing::error!(%error,"Xcos command failed");
                xcss::server_cli::ErrorEnvelope::with_code(
                    xcss::server_cli::ErrorCode::new("xcos.command_failed").expect("static code"),
                    "The command failed. Check the service diagnostic log.",
                )
            };
            xcss::server_cli::report_error(&envelope, json, 1)
        }
    }
}

async fn execute(cli: Cli, log_layer: &xcss::log::XcssStructuredLayer) -> anyhow::Result<()> {
    match cli.command {
        Command::Init { username, settings } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            let settings = loaded.value;
            settings.validate_configuration()?;
            let root = settings.data_directory()?;
            let username = xcss::admin_auth::normalize_administrator_username(&username)?;
            let password = read_password()?;
            xcss::server_cli::create_empty_private_directory(&root).map_err(CliFailure)?;
            let state_directory = xcss::state_file::PrivateStateDirectory::open(&root)?;
            state_directory.verify_no_pending_maintenance()?;
            let _maintenance = state_directory.try_maintenance_lock()?;
            state_directory.verify_no_pending_maintenance()?;
            xcss::server_cli::create_runtime_log_directory(&root).map_err(CliFailure)?;
            let recordings = settings
                .recordings_directory
                .clone()
                .unwrap_or_else(|| root.join("recordings"));
            xcss::state_file::PrivateStateDirectory::create(&recordings)?;
            xcss::server_secrets::initialize(&root, &config::InstanceSecrets::generate()?)?;
            sqlite::initialize_with_administrator(&settings.database()?, &username, &password)
                .await?;
            tracing::info!(event = "common.initialization.completed");
            println!(
                "{}",
                if cli.json {
                    serde_json::json!({"status":"initialized","schema_identity":sqlite::current_schema_identity()?}).to_string()
                } else {
                    "current data initialized".into()
                }
            );
            Ok(())
        }
        Command::Run { settings } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            let config = loaded.value.effective()?;
            let (media_config, media_contract, media_binary) = loaded.value.companion_paths()?;
            let recordings = loaded
                .value
                .recordings_directory
                .clone()
                .unwrap_or(loaded.value.data_directory()?.join("recordings"));
            doctor::verify_companion(
                &media_contract,
                &media_binary,
                &media_config,
                &recordings,
                config.media_callback_bind,
            )
            .await?;
            app::serve(config, log_layer).await
        }
        Command::MediaConfig { settings } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            let value = loaded.value;
            let (path, _, _) = value.companion_paths()?;
            let recordings = value
                .recordings_directory
                .clone()
                .unwrap_or(value.data_directory()?.join("recordings"));
            print!(
                "{}",
                doctor::render_media_config(&path, &recordings, value.media_callback_bind)?
            );
            Ok(())
        }
        Command::Config {
            command: ConfigCommand::Validate { settings },
        } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            loaded.value.validate_configuration()?;
            if cli.json {
                println!(
                    "{}",
                    serde_json::json!({"status":"valid","sources":loaded.sources})
                );
            } else {
                println!("configuration is valid");
            }
            Ok(())
        }
        Command::Check { settings } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            let config = loaded.value.effective()?;
            let root = loaded.value.data_directory()?;
            xcss::state_file::PrivateStateDirectory::open(&root)?;
            xcss::server_cli::validate_runtime_log_directory(&root).map_err(CliFailure)?;
            let database = loaded.value.database()?;
            let recordings = loaded
                .value
                .recordings_directory
                .clone()
                .unwrap_or_else(|| root.join("recordings"));
            xcss::state_file::PrivateStateDirectory::open(&recordings)?;
            let (media_config, media_contract, media_binary) = loaded.value.companion_paths()?;
            doctor::verify_companion(
                &media_contract,
                &media_binary,
                &media_config,
                &recordings,
                config.media_callback_bind,
            )
            .await?;
            let snapshot = sqlite::current_validation_snapshot(&database).await?;
            let validation = async {
                xcss::sqlite::integrity_check(snapshot.pool()).await?;
                xcss::sqlite::foreign_key_check(snapshot.pool()).await?;
                let store =
                    xcss::admin_sqlite::SqliteAdministratorStore::new(snapshot.pool().clone());
                store.validate_all_administrators().await?;
                doctor::verify_credentials_on_snapshot(snapshot.pool(), &config.credentials_key)
                    .await?;
                camera_state::validate_current_camera_data(snapshot.pool()).await?;
                Ok::<_, anyhow::Error>(())
            }
            .await;
            snapshot.close().await;
            validation?;
            let report = doctor::run(&doctor::DoctorOptions {
                database_url: config.database_url,
                mediamtx_config: media_config,
                mediamtx_contract: media_contract,
                mediamtx_binary: media_binary,
                recordings_directory: recordings,
                credentials_key: config.credentials_key,
                media_callback_bind: config.media_callback_bind,
                app_ready_url: format!("http://{}/readyz", config.media_callback_bind),
                mediamtx_ready_url: format!("{}/v3/info", config.mediamtx_api_url),
                offline: true,
            })
            .await?;
            println!("{}", serde_json::to_string(&report)?);
            Ok(())
        }
        Command::Admin {
            command: AdminCommand::ResetPassword { username, settings },
        } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            let config = loaded.value.effective()?;
            let password = read_password()?;
            let _maintenance =
                crate::runtime_lock::DatabaseMaintenanceLock::exclusive(&config.database_url)?;
            sqlite::validate_current_database(&loaded.value.database()?)?;
            let pool = sqlite::open_pool(&config.database_url).await?;
            let service = xcss::admin_core::AdministratorService::new(
                xcss::admin_sqlite::SqliteAdministratorStore::new(pool.clone()),
            );
            let now = u64::try_from(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)?
                    .as_micros(),
            )?;
            let result = service
                .change_administrator_password(&username, &password, now)
                .await;
            pool.close().await;
            result?;
            println!(
                "{}",
                if cli.json {
                    r#"{"status":"password_reset"}"#
                } else {
                    "administrator password reset; sessions revoked"
                }
            );
            Ok(())
        }
        Command::Status { settings } => {
            let loaded = config::load(cli.config.as_deref(), cli.data_dir.as_deref(), &settings)?;
            let report = xcss::server_cli::query_status(loaded.value.media_callback_bind, "xcos")
                .await
                .map_err(CliFailure)?;
            xcss::server_cli::print_report(&report, cli.json)?;
            if !report.ready {
                std::process::exit(1);
            }
            Ok(())
        }
        Command::WebAssets => {
            static_assets::embedded_contract_sha256()?;
            print!("{}", static_assets::MANIFEST);
            Ok(())
        }
        Command::StaticContract => {
            println!("{}", static_assets::embedded_contract_sha256()?);
            Ok(())
        }
        Command::ReleaseIdentity => {
            println!("{}", serde_json::to_string(&release::standard_identity()?)?);
            Ok(())
        }
        Command::StateContract => {
            println!("{}", String::from_utf8(release::state_contract_bytes()?)?);
            Ok(())
        }
        Command::VerifyRelease { release_root } => {
            println!(
                "{}",
                serde_json::to_string(&release::verify_release(&release_root)?)?
            );
            Ok(())
        }
        Command::ReleaseManifestHeader => {
            print!("{}", release::manifest_header()?);
            Ok(())
        }
    }
}

fn read_password() -> anyhow::Result<String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::io::stdin()
        .take((xcss::admin_auth::PASSWORD_MAX_BYTES + 3) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.ends_with(b"\n") {
        bytes.pop();
        if bytes.ends_with(b"\r") {
            bytes.pop();
        }
    }
    let password = String::from_utf8(bytes)?;
    xcss::admin_auth::validate_password(&password)?;
    Ok(password)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_is_global_and_run_does_not_accept_a_release_root() {
        for arguments in [
            vec!["xcos", "--config", "/srv/private/config.json", "run"],
            vec!["xcos", "run", "--config", "/srv/private/config.json"],
            vec!["xcos", "check", "--config", "/srv/private/config.json"],
        ] {
            let cli = Cli::try_parse_from(arguments).unwrap();
            assert_eq!(cli.config, Some("/srv/private/config.json".into()));
        }
        assert!(Cli::try_parse_from(["xcos", "run", "--release-root", "/opt/old"]).is_err());
        assert!(
            Cli::try_parse_from(["xcos", "init", "--username", "admin", "--development"]).is_ok()
        );
    }
}
