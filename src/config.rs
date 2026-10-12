use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

const DEFAULT_BIND_ADDR: &str = "127.0.0.1:18106";

#[derive(Clone)]
pub struct Config {
    pub bind_addr: SocketAddr,
    pub media_callback_bind: SocketAddr,
    pub database_url: String,
    pub jwt_secret: Vec<u8>,
    pub credentials_key: [u8; 32],
    pub development_mode: bool,
    pub media_token_ttl: Duration,
    pub mediamtx_api_url: String,
    pub mediamtx_playback_url: String,
    pub public_webrtc_base_url: String,
    pub public_hls_base_url: String,
    pub public_rtsp_publish_base_url: String,
    pub status_interval: Duration,
    pub reconcile_interval: Duration,
    pub request_timeout: Duration,
    pub static_dir: Option<PathBuf>,
    pub trusted_proxies: Vec<String>,
}

/// The current JSON contract. Secrets never appear in a diagnostic report.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub recordings_directory: Option<PathBuf>,
    pub mediamtx_config: Option<PathBuf>,
    pub mediamtx_contract: Option<PathBuf>,
    pub mediamtx_binary: Option<PathBuf>,
    pub bind: SocketAddr,
    pub media_callback_bind: SocketAddr,
    pub data_dir: Option<PathBuf>,
    pub development: bool,
    pub log_level: String,
    pub media_token_ttl_secs: u64,
    pub mediamtx_api_url: String,
    pub mediamtx_playback_url: String,
    pub public_webrtc_base_url: String,
    pub public_hls_base_url: String,
    pub public_rtsp_publish_base_url: String,
    pub status_interval_secs: u64,
    pub reconcile_interval_secs: u64,
    pub request_timeout_secs: u64,
    pub static_dir: Option<PathBuf>,
    pub trusted_proxies: Vec<String>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            recordings_directory: None,
            mediamtx_config: Some("/etc/isarmg/xcos/mediamtx.yml".into()),
            mediamtx_contract: Some("/opt/isarmg/xcos/config/mediamtx.lock".into()),
            mediamtx_binary: Some("/opt/isarmg/xcos/bin/mediamtx".into()),
            bind: DEFAULT_BIND_ADDR.parse().expect("default address"),
            media_callback_bind: "127.0.0.1:18107".parse().expect("callback address"),
            data_dir: Some("/var/lib/isarmg/xcos".into()),
            development: false,
            log_level: "info".into(),
            media_token_ttl_secs: 120,
            mediamtx_api_url: "http://127.0.0.1:9997".into(),
            mediamtx_playback_url: "http://127.0.0.1:9996".into(),
            public_webrtc_base_url: "/media-webrtc".into(),
            public_hls_base_url: "/media-hls".into(),
            public_rtsp_publish_base_url: "rtsp://127.0.0.1:8554".into(),
            status_interval_secs: 10,
            reconcile_interval_secs: 60,
            request_timeout_secs: 20,
            static_dir: None,
            trusted_proxies: Vec::new(),
        }
    }
}
#[derive(Clone, Default, clap::Args)]
pub struct Overrides {
    #[arg(long)]
    pub bind: Option<SocketAddr>,
    #[arg(long, num_args=0..=1, default_missing_value="true")]
    pub development: Option<bool>,
}

pub fn load(
    file: Option<&Path>,
    data_dir: Option<&Path>,
    cli: &Overrides,
) -> anyhow::Result<xcss::config::Loaded<Settings>> {
    use xcss::config::{EnvMapping, EnvValueKind, Override};
    let mappings = [
        EnvMapping {
            variable: "XCOS_TRUSTED_PROXIES",
            path: "/trusted_proxies",
            kind: EnvValueKind::Json,
        },
        EnvMapping {
            variable: "XCOS_LOG_LEVEL",
            path: "/log_level",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_MEDIA_CALLBACK_BIND",
            path: "/media_callback_bind",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_RECORDINGS_DIR",
            path: "/recordings_directory",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_MEDIAMTX_CONFIG",
            path: "/mediamtx_config",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_MEDIAMTX_CONTRACT",
            path: "/mediamtx_contract",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_MEDIAMTX_BINARY",
            path: "/mediamtx_binary",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_BIND",
            path: "/bind",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_DATA_DIR",
            path: "/data_dir",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_DEVELOPMENT",
            path: "/development",
            kind: EnvValueKind::Boolean,
        },
        EnvMapping {
            variable: "XCOS_MEDIA_TOKEN_TTL_SECS",
            path: "/media_token_ttl_secs",
            kind: EnvValueKind::UnsignedInteger,
        },
        EnvMapping {
            variable: "XCOS_MEDIAMTX_API_URL",
            path: "/mediamtx_api_url",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_MEDIAMTX_PLAYBACK_URL",
            path: "/mediamtx_playback_url",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_PUBLIC_WEBRTC_BASE_URL",
            path: "/public_webrtc_base_url",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_PUBLIC_HLS_BASE_URL",
            path: "/public_hls_base_url",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_PUBLIC_RTSP_PUBLISH_BASE_URL",
            path: "/public_rtsp_publish_base_url",
            kind: EnvValueKind::String,
        },
        EnvMapping {
            variable: "XCOS_STATUS_INTERVAL_SECS",
            path: "/status_interval_secs",
            kind: EnvValueKind::UnsignedInteger,
        },
        EnvMapping {
            variable: "XCOS_RECONCILE_INTERVAL_SECS",
            path: "/reconcile_interval_secs",
            kind: EnvValueKind::UnsignedInteger,
        },
        EnvMapping {
            variable: "XCOS_REQUEST_TIMEOUT_SECS",
            path: "/request_timeout_secs",
            kind: EnvValueKind::UnsignedInteger,
        },
        EnvMapping {
            variable: "XCOS_STATIC_DIR",
            path: "/static_dir",
            kind: EnvValueKind::String,
        },
    ];
    let mut invalid_unicode = false;
    let environment =
        xcss::config::read_environment(&mappings, |name| match std::env::var(name) {
            Ok(value) => Some(value),
            Err(std::env::VarError::NotPresent) => None,
            Err(_) => {
                invalid_unicode = true;
                None
            }
        })?;
    anyhow::ensure!(
        !invalid_unicode,
        "mapped environment settings must be valid Unicode"
    );
    let default_path = Path::new("/etc/isarmg/xcos/config.json");
    let file = match file {
        Some(path) => Some(path),
        None => match std::fs::symlink_metadata(default_path) {
            Ok(_) => Some(default_path),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(error.into()),
        },
    };
    let bytes = file.map(xcss::config::read_private_file).transpose()?;
    let mut explicit = Vec::new();
    if let Some(value) = data_dir {
        explicit.push(Override::new("/data_dir", serde_json::json!(value)));
    }
    if let Some(value) = cli.bind {
        explicit.push(Override::new("/bind", serde_json::json!(value)));
    }
    if let Some(value) = cli.development {
        explicit.push(Override::new("/development", serde_json::json!(value)));
    }
    Ok(xcss::config::resolve_validated(
        &Settings::default(),
        bytes.as_deref(),
        &environment,
        &explicit,
        validate_intrinsic,
    )?)
}

fn validate_intrinsic(
    value: &Settings,
    source: xcss::config::ConfigSource,
) -> Result<(), xcss::config::ConfigError> {
    let invalid =
        |path| xcss::config::ConfigError::new(xcss::config::Reason::InvalidValue, path, source);
    for (field, path) in [
        ("/data_dir", &value.data_dir),
        ("/static_dir", &value.static_dir),
        ("/recordings_directory", &value.recordings_directory),
        ("/mediamtx_config", &value.mediamtx_config),
        ("/mediamtx_contract", &value.mediamtx_contract),
        ("/mediamtx_binary", &value.mediamtx_binary),
    ] {
        if path.as_ref().is_some_and(|path| {
            !path.is_absolute()
                || path.components().any(|c| {
                    matches!(
                        c,
                        std::path::Component::ParentDir | std::path::Component::CurDir
                    )
                })
        }) {
            return Err(invalid(field));
        }
    }
    if value.development && !value.bind.ip().is_loopback() {
        return Err(invalid("/bind"));
    }
    if !value.media_callback_bind.ip().is_loopback() || value.media_callback_bind.port() == 0 {
        return Err(invalid("/media_callback_bind"));
    }
    if value.bind.port() == value.media_callback_bind.port()
        && (value.bind.ip() == value.media_callback_bind.ip() || value.bind.ip().is_unspecified())
    {
        return Err(invalid("/media_callback_bind"));
    }
    if xcss::client_ip::TrustedProxies::parse(&value.trusted_proxies).is_err() {
        return Err(invalid("/trusted_proxies"));
    }
    if !matches!(
        value.log_level.as_str(),
        "error" | "warn" | "info" | "debug" | "trace" | "off"
    ) {
        return Err(invalid("/log_level"));
    }
    for (field, number, min, max) in [
        ("/media_token_ttl_secs", value.media_token_ttl_secs, 30, 300),
        ("/status_interval_secs", value.status_interval_secs, 1, 3600),
        (
            "/reconcile_interval_secs",
            value.reconcile_interval_secs,
            1,
            3600,
        ),
        ("/request_timeout_secs", value.request_timeout_secs, 1, 300),
    ] {
        if !(min..=max).contains(&number) {
            return Err(invalid(field));
        }
    }
    for (field, text) in [
        ("/mediamtx_api_url", &value.mediamtx_api_url),
        ("/mediamtx_playback_url", &value.mediamtx_playback_url),
    ] {
        let url = url::Url::parse(text).map_err(|_| invalid(field))?;
        if url.scheme() != "http"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.host_str().is_some_and(|host| {
                host == "localhost"
                    || host
                        .parse::<std::net::IpAddr>()
                        .is_ok_and(|ip| ip.is_loopback())
            })
        {
            return Err(invalid(field));
        }
    }
    // Origin reachability and required secrets are checked on final effective
    // settings; syntax and secret/interval bounds are checked in every layer.
    if validate_public_rtsp_base(&value.public_rtsp_publish_base_url, true).is_err() {
        return Err(invalid("/public_rtsp_publish_base_url"));
    }
    Ok(())
}

impl Settings {
    pub fn companion_paths(&self) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
        Ok((
            self.mediamtx_config
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MediaMTX config path is required"))?,
            self.mediamtx_contract
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MediaMTX contract path is required"))?,
            self.mediamtx_binary
                .clone()
                .ok_or_else(|| anyhow::anyhow!("MediaMTX binary path is required"))?,
        ))
    }
    pub fn database(&self) -> anyhow::Result<PathBuf> {
        Ok(self.data_directory()?.join("app.sqlite3"))
    }
    pub fn data_directory(&self) -> anyhow::Result<PathBuf> {
        self.data_dir
            .clone()
            .ok_or_else(|| anyhow::anyhow!("data_dir is required"))
    }
    pub fn validate_configuration(&self) -> anyhow::Result<()> {
        validate_intrinsic(self, xcss::config::ConfigSource::File)?;
        validate_development_bind(self.bind, self.development).map_err(anyhow::Error::msg)?;
        self.data_directory()?;
        self.companion_paths()?;
        xcss::client_ip::TrustedProxies::parse(&self.trusted_proxies)?;
        validate_public_rtsp_base(&self.public_rtsp_publish_base_url, self.development)
            .map_err(anyhow::Error::msg)?;
        if self.static_dir.is_some() && !self.development {
            return Err(
                crate::CliFailure(xcss::server_cli::ErrorEnvelope::with_code(
                    xcss::server_cli::ErrorCode::new("invalid_development_override")
                        .expect("static code"),
                    "External Web overrides require explicit loopback development mode.",
                ))
                .into(),
            );
        }
        Ok(())
    }
    pub fn effective(&self) -> anyhow::Result<Config> {
        self.validate_configuration()?;
        let development_mode = self.development;
        let secrets: InstanceSecrets = xcss::server_secrets::load(&self.data_directory()?)?;
        let (jwt_secret, credentials_key) = secrets.decode()?;
        if self.static_dir.is_some() && !development_mode {
            return Err(
                crate::CliFailure(xcss::server_cli::ErrorEnvelope::with_code(
                    xcss::server_cli::ErrorCode::new("invalid_development_override")
                        .expect("static code"),
                    "External Web overrides require explicit loopback development mode.",
                ))
                .into(),
            );
        }
        if let Some(path) = &self.static_dir {
            anyhow::ensure!(path.is_absolute(), "static_dir must be absolute");
        }
        let bounded = |value, min, max| -> anyhow::Result<Duration> {
            anyhow::ensure!(
                (min..=max).contains(&value),
                "configured interval exceeds its current bound"
            );
            Ok(Duration::from_secs(value))
        };
        Ok(Config {
            bind_addr: self.bind,
            media_callback_bind: self.media_callback_bind,
            database_url: format!("sqlite://{}", self.database()?.display()),
            jwt_secret,
            credentials_key,
            development_mode,
            media_token_ttl: bounded(self.media_token_ttl_secs, 30, 300)?,
            mediamtx_api_url: trim_slash(self.mediamtx_api_url.clone()),
            mediamtx_playback_url: trim_slash(self.mediamtx_playback_url.clone()),
            public_webrtc_base_url: trim_slash(self.public_webrtc_base_url.clone()),
            public_hls_base_url: trim_slash(self.public_hls_base_url.clone()),
            public_rtsp_publish_base_url: validate_public_rtsp_base(
                &self.public_rtsp_publish_base_url,
                development_mode,
            )
            .map_err(anyhow::Error::msg)?,
            status_interval: bounded(self.status_interval_secs, 1, 3600)?,
            reconcile_interval: bounded(self.reconcile_interval_secs, 1, 3600)?,
            request_timeout: bounded(self.request_timeout_secs, 1, 300)?,
            static_dir: self.static_dir.clone(),
            trusted_proxies: self.trusted_proxies.clone(),
        })
    }
}

fn validate_public_rtsp_base(value: &str, development_mode: bool) -> Result<String, String> {
    let value = trim_slash(value.to_owned());
    let parsed = url::Url::parse(&value)
        .map_err(|_| "XCOS_PUBLIC_RTSP_PUBLISH_BASE_URL must be a valid RTSP URL".to_owned())?;
    if !matches!(parsed.scheme(), "rtsp" | "rtsps")
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || !matches!(parsed.path(), "" | "/")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return Err(
            "XCOS_PUBLIC_RTSP_PUBLISH_BASE_URL must be an rtsp(s) origin without query or fragment"
                .to_owned(),
        );
    }
    let local_only = parsed.host_str().is_none_or(|host| {
        host.eq_ignore_ascii_case("localhost")
            || host
                .parse::<std::net::IpAddr>()
                .is_ok_and(|address| address.is_loopback() || address.is_unspecified())
    });
    if !development_mode && local_only {
        return Err(
            "XCOS_PUBLIC_RTSP_PUBLISH_BASE_URL must be reachable by paired clients in production"
                .to_owned(),
        );
    }
    if !development_mode && parsed.scheme() != "rtsps" {
        return Err("XCOS_PUBLIC_RTSP_PUBLISH_BASE_URL must use rtsps in production".to_owned());
    }
    Ok(value)
}

#[cfg(test)]
fn absolute_path(name: &str, value: String) -> Result<PathBuf, String> {
    let path = PathBuf::from(value);
    if path.is_absolute() {
        Ok(path)
    } else {
        Err(format!("{name} must be an absolute path"))
    }
}

fn validate_development_bind(bind_addr: SocketAddr, development_mode: bool) -> Result<(), String> {
    if development_mode && !bind_addr.ip().is_loopback() {
        Err("XCOS_BIND must be loopback in development mode".into())
    } else {
        Ok(())
    }
}

/// The callback endpoint is explicit and loopback-only, independent of ingress.
pub(crate) fn listener_addresses(bind: SocketAddr, callback: SocketAddr) -> Vec<SocketAddr> {
    vec![bind, callback]
}

fn trim_slash(mut value: String) -> String {
    while value.ends_with('/') {
        value.pop();
    }
    value
}

/// Private instance state. Never accepted from deployment configuration.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstanceSecrets {
    format: u32,
    jwt_secret: String,
    credentials_key: String,
}
impl InstanceSecrets {
    pub fn generate() -> anyhow::Result<Self> {
        Ok(Self {
            format: 1,
            jwt_secret: STANDARD.encode(xcss::server_secrets::generate_key()?),
            credentials_key: STANDARD.encode(xcss::server_secrets::generate_key()?),
        })
    }
    fn decode(&self) -> anyhow::Result<(Vec<u8>, [u8; 32])> {
        anyhow::ensure!(self.format == 1, "unsupported secrets format");
        let jwt_secret = STANDARD
            .decode(&self.jwt_secret)
            .map_err(|_| anyhow::anyhow!("invalid instance JWT key"))?;
        anyhow::ensure!(jwt_secret.len() == 32, "invalid instance JWT key length");
        let credentials_key: [u8; 32] = STANDARD
            .decode(&self.credentials_key)
            .map_err(|_| anyhow::anyhow!("invalid instance credential key"))?
            .try_into()
            .map_err(|_| anyhow::anyhow!("invalid instance credential key length"))?;
        Ok((jwt_secret, credentials_key))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn development_mode_only_accepts_loopback_bindings() {
        assert!(validate_development_bind("127.0.0.1:18106".parse().unwrap(), true).is_ok());
        assert!(validate_development_bind("[::1]:8080".parse().unwrap(), true).is_ok());
        assert!(validate_development_bind("0.0.0.0:8080".parse().unwrap(), true).is_err());
        assert!(validate_development_bind("192.168.1.10:8080".parse().unwrap(), true).is_err());
        assert!(validate_development_bind("0.0.0.0:8080".parse().unwrap(), false).is_ok());
        assert!(validate_development_bind("192.168.1.10:8080".parse().unwrap(), false).is_ok());
        assert!(validate_development_bind("[fd00::10]:8080".parse().unwrap(), false).is_ok());
    }

    #[test]
    fn configured_ingress_preserves_explicit_callback_without_port_8080() {
        let settings = Settings::default();
        let addresses = listener_addresses(settings.bind, settings.media_callback_bind);
        assert_eq!(
            addresses,
            vec![
                "127.0.0.1:18106".parse().unwrap(),
                "127.0.0.1:18107".parse().unwrap()
            ]
        );
        assert!(addresses.iter().all(|address| address.port() != 8080));
        for address in [
            "0.0.0.0:18107",
            "192.168.1.10:18107",
            "[::]:18107",
            "127.0.0.1:0",
        ] {
            let invalid = Settings {
                media_callback_bind: address.parse().unwrap(),
                ..Settings::default()
            };
            assert!(validate_intrinsic(&invalid, xcss::config::ConfigSource::File).is_err());
        }
    }

    #[test]
    fn production_configuration_accepts_explicit_remote_ingress() {
        for address in ["192.168.1.10:9080", "0.0.0.0:8080", "[fd00::10]:8080"] {
            let settings = Settings {
                bind: address.parse().unwrap(),
                data_dir: Some("/var/lib/isarmg/xcos".into()),
                public_rtsp_publish_base_url: "rtsps://xcos.example.org:8322".into(),
                ..Settings::default()
            };
            let loaded = xcss::config::resolve_validated(
                &Settings::default(),
                Some(&serde_json::to_vec(&settings).unwrap()),
                &[],
                &[],
                validate_intrinsic,
            )
            .unwrap();
            loaded.value.validate_configuration().unwrap();
            assert_eq!(loaded.value.bind, settings.bind);
            assert!(!loaded.value.development);
            let development = Settings {
                development: true,
                ..settings
            };
            assert!(validate_intrinsic(&development, xcss::config::ConfigSource::File).is_err());
            assert!(development.validate_configuration().is_err());
        }
    }

    #[test]
    fn public_json_contract_has_no_legacy_aliases() {
        let value = serde_json::to_value(Settings::default()).unwrap();
        assert_eq!(value["bind"], "127.0.0.1:18106");
        assert_eq!(value["development"], false);
        assert!(value.get("release_root").is_none());
        for legacy in [
            "bind_addr",
            "app_env",
            "release_root",
            "jwt_secret",
            "credentials_key",
            "database_url",
            "runtime_directory",
        ] {
            let mut invalid = value.clone();
            invalid[legacy] = serde_json::json!("old");
            assert!(serde_json::from_value::<Settings>(invalid).is_err());
        }
    }

    #[test]
    fn pure_configuration_does_not_need_or_create_instance_state() {
        let root = tempfile::tempdir().unwrap();
        let missing = root.path().join("not-initialized");
        let settings = Settings {
            data_dir: Some(missing.clone()),
            development: true,
            ..Settings::default()
        };
        settings.validate_configuration().unwrap();
        assert!(!missing.exists());
        assert!(settings.effective().is_err());
        assert!(!missing.exists());
    }

    #[test]
    fn generated_instance_keys_are_private_stable_and_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let settings = Settings {
            data_dir: Some(root.path().into()),
            development: true,
            ..Settings::default()
        };
        let secrets = InstanceSecrets::generate().unwrap();
        xcss::server_secrets::initialize(root.path(), &secrets).unwrap();
        let bytes = std::fs::read(root.path().join("secrets.json")).unwrap();
        assert_eq!(
            std::fs::metadata(root.path().join("secrets.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        let first = settings.effective().unwrap();
        let second = settings.effective().unwrap();
        assert_eq!(first.jwt_secret, second.jwt_secret);
        assert_eq!(first.credentials_key, second.credentials_key);
        assert!(xcss::server_secrets::initialize(
            root.path(),
            &InstanceSecrets::generate().unwrap()
        )
        .is_err());
        assert_eq!(
            bytes,
            std::fs::read(root.path().join("secrets.json")).unwrap()
        );
        std::fs::remove_file(root.path().join("secrets.json")).unwrap();
        assert!(settings.effective().is_err());
        assert!(!root.path().join("secrets.json").exists());
        std::fs::write(root.path().join("secrets.json"), b"{}").unwrap();
        std::fs::set_permissions(
            root.path().join("secrets.json"),
            std::fs::Permissions::from_mode(0o600),
        )
        .unwrap();
        assert!(settings.effective().is_err());
        assert_eq!(
            std::fs::read(root.path().join("secrets.json")).unwrap(),
            b"{}"
        );
    }

    #[test]
    fn an_explicit_missing_config_is_never_ignored() {
        let directory = tempfile::tempdir().unwrap();
        assert!(load(
            Some(&directory.path().join("missing.json")),
            None,
            &Overrides::default()
        )
        .is_err());
    }

    #[test]
    fn companion_paths_do_not_depend_on_install_layout() {
        let settings = Settings {
            mediamtx_config: Some("/srv/custom/media.yml".into()),
            mediamtx_contract: Some("/srv/custom/pinned.lock".into()),
            mediamtx_binary: Some("/srv/custom/media-server".into()),
            ..Settings::default()
        };
        assert_eq!(
            settings.companion_paths().unwrap(),
            (
                "/srv/custom/media.yml".into(),
                "/srv/custom/pinned.lock".into(),
                "/srv/custom/media-server".into()
            )
        );
    }

    #[test]
    fn private_json_file_permissions_are_enforced() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("config.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load(Some(&path), None, &Overrides::default()).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load(Some(&path), None, &Overrides::default()).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(load(Some(&path), None, &Overrides::default()).is_err());
    }

    #[test]
    fn default_server_binding_is_loopback() {
        let address = DEFAULT_BIND_ADDR.parse::<SocketAddr>().unwrap();
        assert!(address.ip().is_loopback());
        assert_eq!(address.port(), 18106);
    }

    #[test]
    fn runtime_paths_are_absolute() {
        assert_eq!(
            absolute_path("XCOS_STATIC_DIR", "/opt/xcos/web".into()).unwrap(),
            PathBuf::from("/opt/xcos/web")
        );
        assert_eq!(
            absolute_path("XCOS_STATIC_DIR", "web/dist".into()).unwrap_err(),
            "XCOS_STATIC_DIR must be an absolute path"
        );
    }

    #[test]
    fn production_rtsp_publish_origin_must_be_client_reachable() {
        assert!(validate_public_rtsp_base("rtsps://xcos.example:8322", false).is_ok());
        assert!(validate_public_rtsp_base("rtsps://192.168.1.20:8322", false).is_ok());
        assert!(validate_public_rtsp_base("rtsp://xcos.example:8554", false).is_err());
        assert!(validate_public_rtsp_base("rtsp://127.0.0.1:8554", false).is_err());
        assert!(validate_public_rtsp_base("rtsp://localhost:8554", false).is_err());
        assert!(validate_public_rtsp_base("rtsp://user:secret@xcos.example:8554", false).is_err());
        assert!(validate_public_rtsp_base("rtsp://xcos.example:8554/path", false).is_err());
        assert!(validate_public_rtsp_base("rtsp://127.0.0.1:8554", true).is_ok());
    }
}
