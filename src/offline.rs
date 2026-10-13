//! Explicit offline setup adapters. The server executable never exposes maintenance commands.
use crate::{camera_state, config, doctor, sqlite, CliFailure};
use anyhow::Context;
use std::path::Path;

fn settings(root: &Path, path: &Path) -> anyhow::Result<config::Settings> {
    Ok(config::load_at_root(root, path)?.value)
}

pub async fn validate_config(
    root: &Path,
    path: &Path,
) -> anyhow::Result<xcss::offline::ConfigReport> {
    let value = settings(root, path)?;
    value.validate_configuration()?;
    let data_dir = value.data_directory()?;
    let recordings = value
        .recordings_directory
        .clone()
        .unwrap_or_else(|| data_dir.join("recordings"));
    let mut state_paths = vec![root.join("config"), root.join("logs"), data_dir.clone()];
    if !state_paths
        .iter()
        .any(|parent| recordings.starts_with(parent))
    {
        state_paths.push(recordings.clone());
    }
    Ok(xcss::offline::ConfigReport {
        product: "xcos".into(),
        root: root.to_path_buf(),
        data_dir: data_dir.clone(),
        state_paths,
        shared_paths: vec![],
        schema_identity: sqlite::current_schema_identity()?,
        bind: value.bind,
        writable_paths: vec![data_dir, recordings, root.join("logs"), root.join("run")],
    })
}

pub async fn initialize(
    root: &Path,
    path: &Path,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let value = settings(root, path)?;
    value.validate_configuration()?;
    let username = xcss::admin_auth::normalize_administrator_username(username)?;
    xcss::admin_auth::validate_password(password)?;
    let data_dir = value.data_directory()?;
    xcss::server_cli::create_empty_private_directory(&data_dir).map_err(CliFailure)?;
    let state = xcss::state_file::PrivateStateDirectory::open(&data_dir)?;
    state.verify_no_pending_maintenance()?;
    let _maintenance = state.try_maintenance_lock()?;
    state.verify_no_pending_maintenance()?;
    for child in ["logs", "run"] {
        xcss::state_file::PrivateStateDirectory::create(root.join(child))?;
    }
    // Check the empty state boundary before creating nested recordings.
    let recordings = value
        .recordings_directory
        .clone()
        .unwrap_or_else(|| data_dir.join("recordings"));
    xcss::state_file::PrivateStateDirectory::create(&recordings)?;
    xcss::server_secrets::initialize(&data_dir, &config::InstanceSecrets::generate()?)?;
    sqlite::initialize_with_administrator(
        &value.database()?,
        &username,
        password,
        &root.join("run"),
    )
    .await?;
    write_media_configuration(root, &value)?;
    Ok(())
}

pub async fn check(root: &Path, path: &Path) -> anyhow::Result<xcss::offline::CheckReport> {
    let report = validate_config(root, path).await?;
    let value = settings(root, path)?;
    let effective = value.effective()?;
    xcss::state_file::PrivateStateDirectory::open(&report.data_dir)?;
    xcss::fs_safety::PrivateDirectory::open_existing(root.join("logs"))?;
    let recordings = value
        .recordings_directory
        .clone()
        .unwrap_or_else(|| report.data_dir.join("recordings"));
    xcss::state_file::PrivateStateDirectory::open(&recordings)?;
    let (media_config, media_contract, media_binary) = value.companion_paths()?;
    verify_media_environment(root)?;
    doctor::verify_companion(
        &media_contract,
        &media_binary,
        &media_config,
        &recordings,
        effective.media_callback_bind,
    )
    .await?;
    let snapshot =
        sqlite::current_validation_snapshot(&value.database()?, &root.join("run")).await?;
    let result = async {
        xcss::sqlite::integrity_check(snapshot.pool()).await?;
        xcss::sqlite::foreign_key_check(snapshot.pool()).await?;
        xcss::admin_sqlite::SqliteAdministratorStore::new(snapshot.pool().clone())
            .validate_all_administrators()
            .await?;
        doctor::verify_credentials_on_snapshot(snapshot.pool(), &effective.credentials_key).await?;
        camera_state::validate_current_camera_data(snapshot.pool()).await?;
        Ok::<_, anyhow::Error>(())
    }
    .await;
    snapshot.close().await;
    result?;
    Ok(xcss::offline::CheckReport {
        config: report,
        initialized: true,
    })
}

pub async fn reset_password(
    root: &Path,
    path: &Path,
    username: &str,
    password: &str,
) -> anyhow::Result<()> {
    let value = settings(root, path)?;
    value.validate_configuration()?;
    let data_dir = value.data_directory()?;
    let state = xcss::state_file::PrivateStateDirectory::open(&data_dir)?;
    let _maintenance = state.try_maintenance_lock()?;
    state.verify_no_pending_maintenance()?;
    let pool = sqlite::open_pool(
        &format!("sqlite://{}", value.database()?.display()),
        &root.join("run"),
    )
    .await?;
    let service = xcss::admin_core::AdministratorService::new(
        xcss::admin_sqlite::SqliteAdministratorStore::new(pool.clone()),
    );
    let now = u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_micros(),
    )?;
    let result = service
        .change_administrator_password(username, password, now)
        .await;
    pool.close().await;
    result?;
    Ok(())
}

pub async fn regenerate_media_config(root: &Path, path: &Path) -> anyhow::Result<()> {
    let value = settings(root, path)?;
    value.validate_configuration()?;
    let state = xcss::state_file::PrivateStateDirectory::open(value.data_directory()?)?;
    state.verify_no_pending_maintenance()?;
    let _maintenance = state.try_maintenance_lock()?;
    state.verify_no_pending_maintenance()?;
    write_media_configuration(root, &value)
}

fn write_media_configuration(root: &Path, value: &config::Settings) -> anyhow::Result<()> {
    value.validate_configuration()?;
    let recordings = value.recordings_directory.clone().unwrap_or_else(|| {
        value
            .data_directory()
            .expect("validated directory")
            .join("recordings")
    });
    let content = doctor::render_media_config(
        &root.join("share/mediamtx.yml"),
        &recordings,
        value.media_callback_bind,
    )?;
    let environment = if root.join("config/mediamtx.env").try_exists()? {
        parse_media_environment(root)?
    } else {
        let public = url::Url::parse(&value.public_rtsp_publish_base_url)?;
        let host = public
            .host()
            .context("media public host is required")?
            .to_string();
        std::collections::BTreeMap::from([
            ("MTX_WEBRTCADDITIONALHOSTS".to_owned(), host),
            ("MTX_RTSPENCRYPTION".to_owned(), "strict".to_owned()),
            ("MTX_RTSPSADDRESS".to_owned(), ":8322".to_owned()),
            (
                "MTX_RTSPSERVERCERT".to_owned(),
                root.join("config/rtsp-cert.pem")
                    .to_str()
                    .context("certificate path must be UTF-8")?
                    .to_owned(),
            ),
            (
                "MTX_RTSPSERVERKEY".to_owned(),
                root.join("config/rtsp-key.pem")
                    .to_str()
                    .context("key path must be UTF-8")?
                    .to_owned(),
            ),
            (
                "MTX_PATHDEFAULTS_RECORDDELETEAFTER".to_owned(),
                "168h".to_owned(),
            ),
        ])
    };
    validate_media_environment(root, &environment)?;
    let escape = |value: &str| value.replace('\\', r"\\").replace('"', r#"\""#);
    let encoded = environment
        .iter()
        .map(|(key, value)| format!("{key}=\"{}\"\n", escape(value)))
        .collect::<String>();
    let directory = xcss::fs_safety::PrivateDirectory::open_existing(root.join("config"))?;
    xcss::fs_safety::AtomicFile::replace(
        &directory,
        &xcss::fs_safety::RelativePath::new("mediamtx.env")?,
        encoded.as_bytes(),
    )?;
    xcss::fs_safety::AtomicFile::replace(
        &directory,
        &xcss::fs_safety::RelativePath::new("mediamtx.yml")?,
        content.as_bytes(),
    )?;
    Ok(())
}

fn parse_media_environment(
    root: &Path,
) -> anyhow::Result<std::collections::BTreeMap<String, String>> {
    let bytes = xcss::config::read_private_file(&root.join("config/mediamtx.env"))?;
    anyhow::ensure!(
        bytes.len() <= 16 * 1024,
        "media environment exceeds its bound"
    );
    let text = std::str::from_utf8(&bytes)?;
    let mut values = std::collections::BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
    {
        let (key, input) = line
            .split_once('=')
            .context("media environment requires key=value")?;
        let value =
            if let Some(quoted) = input.strip_prefix('"') {
                let quoted = quoted
                    .strip_suffix('"')
                    .context("invalid quoted media value")?;
                let mut value = String::new();
                let mut characters = quoted.chars();
                while let Some(character) = characters.next() {
                    if character == '\\' {
                        let escaped = characters.next().context("incomplete media escape")?;
                        anyhow::ensure!(matches!(escaped, '\\' | '"'), "unsupported media escape");
                        value.push(escaped);
                    } else {
                        anyhow::ensure!(character != '"', "unescaped media quote");
                        value.push(character);
                    }
                }
                value
            } else {
                anyhow::ensure!(
                    !input.chars().any(|character| character.is_whitespace()
                        || matches!(character, '"' | '\\' | '\'')),
                    "invalid unquoted media value"
                );
                input.to_owned()
            };
        anyhow::ensure!(
            values.insert(key.to_owned(), value).is_none(),
            "duplicate media environment key"
        );
    }
    validate_media_environment(root, &values)?;
    Ok(values)
}

fn validate_media_environment(
    root: &Path,
    values: &std::collections::BTreeMap<String, String>,
) -> anyhow::Result<()> {
    const KEYS: [&str; 6] = [
        "MTX_WEBRTCADDITIONALHOSTS",
        "MTX_RTSPENCRYPTION",
        "MTX_RTSPSADDRESS",
        "MTX_RTSPSERVERCERT",
        "MTX_RTSPSERVERKEY",
        "MTX_PATHDEFAULTS_RECORDDELETEAFTER",
    ];
    anyhow::ensure!(
        values.len() == KEYS.len() && KEYS.iter().all(|key| values.contains_key(*key)),
        "media environment must contain exactly the managed key set"
    );
    anyhow::ensure!(
        values
            .values()
            .all(|value| !value.is_empty() && !value.chars().any(char::is_control)),
        "media values must be bounded nonempty text"
    );
    anyhow::ensure!(
        values["MTX_RTSPENCRYPTION"] == "strict",
        "media requires strict RTSPS"
    );
    anyhow::ensure!(
        Path::new(&values["MTX_RTSPSERVERCERT"]) == root.join("config/rtsp-cert.pem")
            && Path::new(&values["MTX_RTSPSERVERKEY"]) == root.join("config/rtsp-key.pem"),
        "media certificates must use the fixed ROOT/config paths"
    );
    let address = &values["MTX_RTSPSADDRESS"];
    let socket = if address.starts_with(':') {
        format!("0.0.0.0{address}")
    } else {
        address.clone()
    };
    anyhow::ensure!(
        socket.parse::<std::net::SocketAddr>()?.port() != 0,
        "media requires a concrete RTSPS port"
    );
    validate_retention(&values["MTX_PATHDEFAULTS_RECORDDELETEAFTER"])?;
    for host in values["MTX_WEBRTCADDITIONALHOSTS"].split(',') {
        anyhow::ensure!(
            !host.is_empty() && !host.chars().any(char::is_whitespace),
            "media ICE host is invalid"
        );
        if host.parse::<std::net::IpAddr>().is_err() {
            let url = url::Url::parse(&format!("https://{host}"))?;
            anyhow::ensure!(
                url.host_str() == Some(host)
                    && url.username().is_empty()
                    && url.password().is_none()
                    && url.port().is_none()
                    && url.path() == "/"
                    && url.query().is_none()
                    && url.fragment().is_none(),
                "media ICE hostname is invalid"
            );
        }
    }
    Ok(())
}

fn validate_retention(value: &str) -> anyhow::Result<()> {
    // MediaMTX uses nonnegative native durations (including an initial day component), with zero disabling expiry.
    // Keep that current business option without introducing a second syntax.
    if value == "0" {
        return Ok(());
    }
    anyhow::ensure!(
        !value.is_empty() && value.len() <= 256,
        "recording retention exceeds its text bound"
    );
    let mut remaining = value;
    let mut total = 0_u128;
    let mut first = true;
    while !remaining.is_empty() {
        let split = remaining
            .char_indices()
            .find(|(_, character)| !character.is_ascii_digit() && *character != '.')
            .map_or(remaining.len(), |(index, _)| index);
        let number = &remaining[..split];
        remaining = &remaining[split..];
        anyhow::ensure!(!number.is_empty(), "retention requires a number and unit");
        let (unit, scale) = [
            ("ns", 1_u128),
            ("us", 1_000),
            ("µs", 1_000),
            ("μs", 1_000),
            ("ms", 1_000_000),
            ("s", 1_000_000_000),
            ("m", 60_000_000_000),
            ("h", 3_600_000_000_000),
            ("d", 86_400_000_000_000),
        ]
        .into_iter()
        .find(|(unit, _)| remaining.starts_with(*unit))
        .context("unsupported retention unit")?;
        remaining = &remaining[unit.len()..];
        if unit == "d" {
            anyhow::ensure!(
                first && !number.contains('.'),
                "days are only allowed as the first integer component"
            );
        }
        first = false;
        let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
        anyhow::ensure!(
            !whole.is_empty() || !fraction.is_empty(),
            "invalid retention number"
        );
        let whole = if whole.is_empty() {
            0
        } else {
            whole.parse::<u128>()?
        };
        anyhow::ensure!(
            fraction.bytes().all(|byte| byte.is_ascii_digit()),
            "invalid retention fraction"
        );
        let fraction = &fraction[..fraction.len().min(18)];
        let partial = if fraction.is_empty() {
            0
        } else {
            fraction.parse::<u128>()? * scale / 10_u128.pow(u32::try_from(fraction.len())?)
        };
        total = total
            .checked_add(whole.checked_mul(scale).context("retention overflow")?)
            .and_then(|value| value.checked_add(partial))
            .context("retention overflow")?;
        anyhow::ensure!(
            total <= i64::MAX as u128,
            "retention exceeds the native duration bound"
        );
    }
    anyhow::ensure!(
        total == 0 || total >= 15 * 60 * 1_000_000_000,
        "retention cannot be shorter than the fixed fifteen-minute recording segment"
    );
    Ok(())
}

pub(crate) fn verify_media_environment(root: &Path) -> anyhow::Result<()> {
    parse_media_environment(root)?;
    Ok(())
}

pub async fn migrate(
    root: &Path,
    config: &Path,
    data_dir: &Path,
    source: &xcss::contracts::SchemaIdentity,
    target: &xcss::contracts::SchemaIdentity,
) -> anyhow::Result<()> {
    let report = validate_config(root, config).await?;
    anyhow::ensure!(
        source == target && target == &report.schema_identity,
        "Xcos does not support the requested schema migration"
    );
    anyhow::ensure!(
        data_dir == report.data_dir,
        "migration data_dir differs from the locked instance"
    );
    check(root, config).await?;
    Ok(())
}
