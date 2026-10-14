use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Clone)]
pub struct Config {
    pub root: PathBuf,
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
}

/// The current JSON contract. Secrets never appear in a diagnostic report.
pub use crate::config_definition::Settings;
#[cfg(test)]
use crate::config_definition::DEFAULT_BIND_ADDR;

pub fn load_at_root(root: &Path, file: &Path) -> anyhow::Result<xcss::config::Loaded<Settings>> {
    validate_root(root)?;
    use xcss::config::{EnvMapping, EnvValueKind};
    let mappings = [
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
    let bytes = xcss::config::read_managed_file(
        file,
        cfg!(debug_assertions) && env!("XCOS_SOURCE_REVISION") == "unbound",
    )?;
    let defaults = Settings {
        root: root.to_path_buf(),
        data_dir: Some(root.join("state")),
        mediamtx_config: Some(root.join("config/mediamtx.yml")),
        mediamtx_contract: Some(root.join("share/mediamtx.lock")),
        mediamtx_binary: Some(root.join("bin/mediamtx")),
        ..Default::default()
    };
    let mut loaded = xcss::config::resolve_validated(
        &defaults,
        Some(&bytes),
        &environment,
        &[],
        |value, source| {
            let mut effective = value.clone();
            effective.root = root.to_path_buf();
            validate_intrinsic(&effective, source)
        },
    )?;
    loaded.value.root = root.to_path_buf();
    Ok(loaded)
}

pub(crate) fn validate_root(root: &Path) -> anyhow::Result<()> {
    anyhow::ensure!(
        root.is_absolute()
            && root != Path::new("/")
            && !root.components().any(|part| matches!(
                part,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )),
        "root must be a normalized absolute directory"
    );
    Ok(())
}

fn validate_intrinsic(
    value: &Settings,
    source: xcss::config::ConfigSource,
) -> Result<(), xcss::config::ConfigError> {
    xcss::configuration_contract::validate_rules(
        &crate::config_definition::current_rules(
            false,
            !cfg!(debug_assertions) || env!("XCOS_SOURCE_REVISION") != "unbound",
        ),
        &serde_json::to_value(value).map_err(|_| {
            xcss::config::ConfigError::new(xcss::config::Reason::TypeMismatch, "", source)
        })?,
        &value.root,
        source,
    )
}

impl Settings {
    pub fn companion_paths(&self) -> anyhow::Result<(PathBuf, PathBuf, PathBuf)> {
        let value = serde_json::to_value(self)?;
        for (pointer, relative) in crate::config_definition::fixed_companion_paths() {
            let selected = value
                .pointer(pointer)
                .and_then(serde_json::Value::as_str)
                .ok_or_else(|| anyhow::anyhow!("MediaMTX file path is required"))?;
            anyhow::ensure!(
                Path::new(selected) == self.root.join(relative),
                "MediaMTX files must use the fixed ROOT layout"
            );
        }
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
        xcss::configuration_contract::validate_rules(
            &crate::config_definition::current_rules(
                true,
                !cfg!(debug_assertions) || env!("XCOS_SOURCE_REVISION") != "unbound",
            ),
            &serde_json::to_value(self)?,
            &self.root,
            xcss::config::ConfigSource::File,
        )?;
        if self.static_dir.is_some()
            && (!self.development
                || !cfg!(debug_assertions)
                || env!("XCOS_SOURCE_REVISION") != "unbound")
        {
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
            root: self.root.clone(),
            bind_addr: self.bind,
            media_callback_bind: self.media_callback_bind,
            database_url: format!("sqlite://{}", self.database()?.display()),
            jwt_secret,
            credentials_key,
            development_mode,
            media_token_ttl: bounded(self.media_token_ttl_secs, 30, 300)?,
            mediamtx_api_url: crate::config_definition::MEDIA_API_ORIGIN.into(),
            mediamtx_playback_url: crate::config_definition::MEDIA_PLAYBACK_ORIGIN.into(),
            public_webrtc_base_url: trim_slash(self.public_webrtc_base_url.clone()),
            public_hls_base_url: trim_slash(self.public_hls_base_url.clone()),
            public_rtsp_publish_base_url: trim_slash(self.public_rtsp_publish_base_url.clone()),
            status_interval: bounded(self.status_interval_secs, 1, 3600)?,
            reconcile_interval: bounded(self.reconcile_interval_secs, 1, 3600)?,
            request_timeout: bounded(self.request_timeout_secs, 1, 300)?,
            static_dir: self.static_dir.clone(),
        })
    }
}

#[cfg(test)]
fn validate_public_rtsp_base(value: &str, development_mode: bool) -> Result<String, String> {
    let settings = Settings {
        public_rtsp_publish_base_url: value.into(),
        development: development_mode,
        ..Settings::default()
    };
    settings
        .validate_configuration()
        .map_err(|error| error.to_string())?;
    Ok(trim_slash(value.to_owned()))
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

use crate::secrets_definition::{InstanceSecrets, FORMAT as SECRET_FORMAT, KEY_BYTES};
impl InstanceSecrets {
    #[cfg(test)]
    pub fn generate() -> anyhow::Result<Self> {
        Ok(Self {
            format: SECRET_FORMAT,
            jwt_secret: STANDARD.encode(xcss::server_secrets::generate_key()?),
            credentials_key: STANDARD.encode(xcss::server_secrets::generate_key()?),
        })
    }
    fn decode(&self) -> anyhow::Result<(Vec<u8>, [u8; 32])> {
        anyhow::ensure!(self.format == SECRET_FORMAT, "unsupported secrets format");
        let jwt_secret = STANDARD
            .decode(&self.jwt_secret)
            .map_err(|_| anyhow::anyhow!("invalid instance JWT key"))?;
        anyhow::ensure!(
            jwt_secret.len() == KEY_BYTES,
            "invalid instance JWT key length"
        );
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
                data_dir: Some("/opt/isarmg/xcos/state".into()),
                public_rtsp_publish_base_url: "rtsps://xcos.example.org:8322".into(),
                ..Settings::default()
            };
            let mut loaded = xcss::config::resolve_validated(
                &Settings::default(),
                Some(&serde_json::to_vec(&settings).unwrap()),
                &[],
                &[],
                |value, source| {
                    let mut bound = value.clone();
                    bound.root = settings.root.clone();
                    validate_intrinsic(&bound, source)
                },
            )
            .unwrap();
            loaded.value.root = settings.root.clone();
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
        assert!(load_at_root(directory.path(), &directory.path().join("missing.json")).is_err());
    }

    #[test]
    fn companion_files_are_bound_to_the_selected_root() {
        let root = PathBuf::from("/srv/custom/xcos");
        let settings = Settings {
            root: root.clone(),
            mediamtx_config: Some(root.join("config/mediamtx.yml")),
            mediamtx_contract: Some(root.join("share/mediamtx.lock")),
            mediamtx_binary: Some(root.join("bin/mediamtx")),
            ..Settings::default()
        };
        assert_eq!(
            settings.companion_paths().unwrap().2,
            root.join("bin/mediamtx")
        );
        let invalid = Settings {
            mediamtx_binary: Some("/usr/bin/mediamtx".into()),
            ..settings
        };
        assert!(invalid.companion_paths().is_err());
    }

    #[test]
    fn private_json_file_permissions_are_enforced() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let path = directory.path().join("config.json");
        std::fs::write(&path, b"{}").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert!(load_at_root(directory.path(), &path).is_ok());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(load_at_root(directory.path(), &path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(load_at_root(directory.path(), &path).is_err());
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
