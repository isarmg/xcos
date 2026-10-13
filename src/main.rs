use clap::Parser;
use std::path::PathBuf;

#[derive(Parser)]
#[command(name = "xcos", version, about = "Xcos camera monitoring control plane")]
struct Cli {
    #[arg(long, global = true, default_value = "/opt/isarmg/xcos")]
    root: PathBuf,
}

#[tokio::main]
async fn main() -> std::process::ExitCode {
    let cli = Cli::parse();
    match xcos::run(&cli.root).await {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            tracing::error!(%error, "Xcos startup failed");
            let envelope = if let Some(error) = error.downcast_ref::<xcss::config::ConfigError>() {
                error.envelope()
            } else if let Some(error) = error.downcast_ref::<xcss::state_file::Error>() {
                xcss::server_cli::state_error(error)
            } else if let Some(error) = error.downcast_ref::<xcss::sqlite::SnapshotError>() {
                xcss::server_cli::snapshot_error(error)
            } else {
                xcss::server_cli::ErrorEnvelope::with_code(
                    xcss::server_cli::ErrorCode::new("xcos.startup_failed").expect("static code"),
                    "The service could not start. Check configuration and service diagnostics.",
                )
            };
            xcss::server_cli::report_error(&envelope, false, 1)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn server_accepts_only_run_and_root() {
        assert!(Cli::try_parse_from(["xcos", "--root", "/srv/xcos"]).is_ok());
        for command in [
            "init",
            "check",
            "status",
            "admin",
            "config",
            "media-config",
            "release-identity",
            "verify-release",
        ] {
            assert!(Cli::try_parse_from(["xcos", command]).is_err());
        }
        assert!(Cli::try_parse_from(["xcos", "--config", "/tmp/config.json"]).is_err());
    }
}
