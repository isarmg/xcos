use crate::{crypto::SecretBox, protocol::CONTRACT, sqlite};
use anyhow::{ensure, Context};
use futures_util::TryStreamExt;
use sha2::{Digest, Sha256};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::Read,
    path::{Component, Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
struct ParsedContract {
    version: String,
    platform: String,
    sha256: String,
}

pub(crate) async fn verify_credentials_on_snapshot(
    pool: &sqlx::SqlitePool,
    key: &[u8; 32],
) -> anyhow::Result<()> {
    let secret_box = SecretBox::new(key);
    let columns = sqlite::bounded_columns(&[
        ("id", 36),
        ("authorization_code_enc", 1024),
        ("authorization_code_hash", 32),
    ]);
    // SQLite permits both text UUIDs and SQLx's native 16-byte UUID blobs in
    // this column. Read bounded bytes so both storage forms authenticate.
    let sql = format!(
        "SELECT CAST(id AS BLOB),authorization_code_enc,authorization_code_hash FROM \
         (SELECT {columns} FROM xcocs)"
    );
    // Both identifiers and length guards come from product constants; all
    // stored values remain query results, never executable SQL input.
    let mut rows =
        sqlx::query_as::<_, (Vec<u8>, Vec<u8>, Vec<u8>)>(sqlx::AssertSqlSafe(sql)).fetch(pool);
    while let Some((id, envelope, stored_hash)) = rows.try_next().await? {
        let id = if id.len() == 16 {
            Uuid::from_slice(&id)
        } else {
            Uuid::try_parse_ascii(&id)
        }
        .map_err(|_| anyhow::anyhow!("camera instance identity is invalid"))?;
        let code = secret_box
            .decrypt_client_authorization(&id.to_string(), &envelope)
            .map_err(|_| {
                anyhow::anyhow!(
                    "credentials_key cannot authenticate current camera authorization envelopes"
                )
            })?;
        ensure!(
            crate::crypto::is_current_authorization_code(&code)
                && Sha256::digest(code.as_bytes()).as_slice() == stored_hash,
            "camera authorization code or digest is invalid"
        );
    }
    Ok(())
}

pub(crate) async fn verify_companion(
    contract_path: &Path,
    binary_path: &Path,
    config_path: &Path,
    recordings_directory: &Path,
    callback: std::net::SocketAddr,
) -> anyhow::Result<()> {
    require_secure_file(contract_path, "MediaMTX contract")?;
    require_secure_file(binary_path, "MediaMTX binary")?;
    require_secure_file(config_path, "MediaMTX config")?;
    let directory = xcss::state_file::PrivateStateDirectory::open(recordings_directory)?;

    let contract = parse_contract(contract_path)?;
    let supported_version = include_str!("../config/mediamtx.lock")
        .lines()
        .find_map(|line| line.strip_prefix("version="))
        .expect("embedded companion version");
    ensure!(
        contract.version == supported_version,
        "MediaMTX version is not supported by this application"
    );
    ensure!(
        contract.platform == "linux_amd64",
        "MediaMTX companion platform is unsupported"
    );
    ensure!(
        sha256_file(binary_path)? == contract.sha256,
        "MediaMTX binary hash does not match its contract"
    );
    let stdout = bounded_companion_version(binary_path).await?;
    let version = String::from_utf8(stdout).context("MediaMTX version is not UTF-8")?;
    ensure!(
        version.trim() == contract.version,
        "MediaMTX binary version does not match its contract"
    );
    let content = String::from_utf8(read_limited(config_path, 1024 * 1024)?)?;
    verify_media_config_content(&content, recordings_directory, callback)?;
    directory.verify_identity()?;
    Ok(())
}

async fn bounded_companion_version(binary_path: &Path) -> anyhow::Result<Vec<u8>> {
    use tokio::io::{AsyncRead, AsyncReadExt};
    async fn read_version_pipe(reader: impl AsyncRead + Unpin) -> anyhow::Result<Vec<u8>> {
        let mut bytes = Vec::with_capacity(257);
        reader.take(257).read_to_end(&mut bytes).await?;
        ensure!(
            bytes.len() <= 256,
            "MediaMTX version output exceeds its limit"
        );
        Ok(bytes)
    }
    let mut child = tokio::process::Command::new(binary_path)
        .arg("--version")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .context("execute MediaMTX version check")?;
    let stdout = child
        .stdout
        .take()
        .context("MediaMTX stdout is unavailable")?;
    let stderr = child
        .stderr
        .take()
        .context("MediaMTX stderr is unavailable")?;
    let result = tokio::time::timeout(Duration::from_secs(3), async {
        tokio::try_join!(
            read_version_pipe(stdout),
            read_version_pipe(stderr),
            async { child.wait().await.map_err(anyhow::Error::from) }
        )
    })
    .await;
    match result {
        Ok(Ok((stdout, _, status))) if status.success() => Ok(stdout),
        failed => {
            // Reap on every failure; a caller cannot leave an unbounded version probe running.
            let _ = child.kill().await;
            let _ = child.wait().await;
            match failed {
                Err(_) => anyhow::bail!("MediaMTX version check timed out"),
                Ok(Err(error)) => Err(error),
                Ok(Ok(_)) => anyhow::bail!("MediaMTX version check failed"),
            }
        }
    }
}

fn parse_contract(path: &Path) -> anyhow::Result<ParsedContract> {
    let content = String::from_utf8(read_limited(path, 16 * 1024)?)
        .context("MediaMTX contract is not UTF-8")?;
    let mut values = BTreeMap::new();
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line
            .split_once('=')
            .context("MediaMTX contract contains an invalid line")?;
        ensure!(
            matches!(key, "version" | "platform" | "sha256"),
            "MediaMTX contract contains an unknown field"
        );
        ensure!(
            values.insert(key, value.trim()).is_none(),
            "MediaMTX contract contains a duplicate field"
        );
    }
    ensure!(values.len() == 3, "MediaMTX contract is incomplete");
    let version = values["version"].to_string();
    let platform = values["platform"].to_string();
    let sha256 = values["sha256"].to_ascii_lowercase();
    ensure!(!version.is_empty(), "MediaMTX version is missing");
    ensure!(
        sha256.len() == 64
            && sha256
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "MediaMTX SHA-256 must be lowercase hexadecimal"
    );
    Ok(ParsedContract {
        version,
        platform,
        sha256,
    })
}

/// Render only deployment-owned callback/storage values. TLS and ICE settings
/// remain in the reviewed template and the companion's own environment.
pub(crate) fn render_media_config(
    path: &Path,
    recordings: &Path,
    callback: std::net::SocketAddr,
) -> anyhow::Result<String> {
    ensure!(
        callback.ip().is_loopback() && callback.port() != 0,
        "media callback must be a loopback socket"
    );
    let content = String::from_utf8(read_limited(path, 1024 * 1024)?)
        .context("MediaMTX config is not UTF-8")?;
    let record_path = format!("{}/%path/%Y-%m-%d_%H-%M-%S-%f", recordings.display());
    verify_record_path(&record_path, recordings)?;
    for key in ["authHTTPAddress", "recordPath"] {
        ensure!(
            config_values(&content, key).len() == 1,
            "MediaMTX template must declare exactly one {key}"
        );
    }
    let mut generated = String::new();
    for line in content.lines() {
        let indentation = &line[..line.len() - line.trim_start().len()];
        if line.trim_start().starts_with("authHTTPAddress:") {
            generated.push_str(&format!(
                "{indentation}authHTTPAddress: http://{callback}{}\n",
                CONTRACT.media_auth_path
            ));
        } else if line.trim_start().starts_with("recordPath:") {
            // JSON strings are valid YAML quoted scalars, including unusual path characters.
            generated.push_str(&format!(
                "{indentation}recordPath: {}\n",
                serde_json::to_string(&record_path)?
            ));
        } else {
            generated.push_str(line);
            generated.push('\n');
        }
    }
    verify_media_config_content(&generated, recordings, callback)?;
    Ok(generated)
}

#[cfg(test)]
const MANAGED_MEDIA_CONFIG: &str = include_str!("../config/mediamtx.yml");
#[cfg(test)]
const MANAGED_RECORD_PATH: &str = "/opt/isarmg/xcos/state/recordings/%path/%Y-%m-%d_%H-%M-%S-%f";

fn verify_media_config_content(
    content: &str,
    recordings_directory: &Path,
    callback: std::net::SocketAddr,
) -> anyhow::Result<()> {
    let auth_address = format!("http://{callback}{}", CONTRACT.media_auth_path);
    for (key, expected) in [
        ("authMethod", "http"),
        ("authHTTPAddress", auth_address.as_str()),
        ("apiAddress", "127.0.0.1:9997"),
        ("playbackAddress", "127.0.0.1:9996"),
        ("recordFormat", "fmp4"),
    ] {
        let values = config_values(content, key);
        ensure!(
            values.len() == 1 && values[0].trim_matches(['\'', '"']) == expected,
            "MediaMTX config has a missing, duplicate or invalid companion setting"
        );
    }
    let paths = config_values(content, "recordPath");
    ensure!(
        paths.len() == 1,
        "MediaMTX config must declare exactly one recordPath"
    );
    let configured = paths[0];
    let decoded: String;
    let configured = if configured.starts_with('"') {
        decoded = serde_json::from_str(configured)?;
        decoded.as_str()
    } else {
        configured.trim_matches('\'')
    };
    verify_record_path(configured, recordings_directory)?;
    Ok(())
}

fn verify_record_path(configured: &str, recordings_directory: &Path) -> anyhow::Result<()> {
    let (root, suffix) = configured
        .split_once("/%path/")
        .context("MediaMTX recordPath must contain one %path directory component")?;
    ensure!(
        !suffix.is_empty()
            && !suffix.contains("%path")
            && suffix
                .split('/')
                .all(|part| !matches!(part, "" | "." | "..")),
        "MediaMTX recordPath has an invalid path component"
    );
    ensure!(!root.is_empty(), "MediaMTX recordPath has an empty root");
    let root = PathBuf::from(root);
    ensure!(root.is_absolute(), "MediaMTX recordPath must be absolute");
    ensure!(
        root == recordings_directory,
        "MediaMTX recordPath points at a different recordings directory"
    );
    ensure!(
        root.components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_))),
        "recording path must be normalized"
    );
    Ok(())
}

fn config_values<'a>(content: &'a str, key: &str) -> Vec<&'a str> {
    let prefix = format!("{key}:");
    content
        .lines()
        .filter_map(|line| line.trim().strip_prefix(&prefix).map(str::trim))
        .collect()
}

fn sha256_file(path: &Path) -> anyhow::Result<String> {
    let mut file = open_read_no_follow(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn read_limited(path: &Path, limit: u64) -> anyhow::Result<Vec<u8>> {
    require_secure_file(path, "input file")?;
    let before = fs::symlink_metadata(path)?;
    ensure!(before.len() <= limit, "input file exceeds its size limit");
    let mut output = Vec::with_capacity(before.len() as usize);
    open_read_no_follow(path)?.read_to_end(&mut output)?;
    let after = fs::symlink_metadata(path)?;
    ensure!(
        stable_metadata(&before, &after) && output.len() as u64 == after.len(),
        "input file changed while it was read"
    );
    Ok(output)
}

fn require_secure_file(path: &Path, description: &str) -> anyhow::Result<()> {
    reject_symlink_components(path)?;
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("{description} does not exist: {}", path.display()))?;
    ensure!(metadata.is_file(), "{description} must be a regular file");
    #[cfg(unix)]
    ensure!(
        metadata.nlink() == 1,
        "{description} must not have hard-link aliases"
    );
    Ok(())
}

fn reject_symlink_components(path: &Path) -> anyhow::Result<()> {
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Prefix(prefix) => current.push(prefix.as_os_str()),
            Component::RootDir => current.push(Path::new(std::path::MAIN_SEPARATOR_STR)),
            Component::CurDir => current.push("."),
            Component::Normal(value) => current.push(value),
            Component::ParentDir => anyhow::bail!("operational path must not contain traversal"),
        }
        let metadata = fs::symlink_metadata(&current)
            .with_context(|| format!("operational path does not exist: {}", current.display()))?;
        ensure!(
            !metadata.file_type().is_symlink(),
            "symbolic links are not accepted in operational paths"
        );
    }
    Ok(())
}

fn open_read_no_follow(path: &Path) -> anyhow::Result<File> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::CLOEXEC).bits() as i32);
    options
        .open(path)
        .with_context(|| format!("open {}", path.display()))
}

fn stable_metadata(before: &fs::Metadata, after: &fs::Metadata) -> bool {
    if before.len() != after.len() || before.modified().ok() != after.modified().ok() {
        return false;
    }
    #[cfg(unix)]
    {
        before.dev() == after.dev()
            && before.ino() == after.ino()
            && before.mtime_nsec() == after.mtime_nsec()
    }
    #[cfg(not(unix))]
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn credential_validation_accepts_native_uuid_storage_and_rejects_wrong_keys() {
        let pool = sqlx::sqlite::SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::query(
            "CREATE TABLE xcocs(id TEXT PRIMARY KEY, authorization_code_enc BLOB NOT NULL, authorization_code_hash BLOB NOT NULL)",
        )
        .execute(&pool)
        .await
        .unwrap();
        let id = Uuid::new_v4();
        let key = [0x42; 32];
        let envelope = SecretBox::new(&key)
            .encrypt_client_authorization(&id.to_string(), &"a".repeat(36))
            .unwrap();
        // Match the pairing route: SQLx binds UUIDs as 16-byte SQLite blobs.
        sqlx::query("INSERT INTO xcocs VALUES (?, ?, ?)")
            .bind(id)
            .bind(envelope)
            .bind(Sha256::digest("a".repeat(36).as_bytes()).to_vec())
            .execute(&pool)
            .await
            .unwrap();
        verify_credentials_on_snapshot(&pool, &key).await.unwrap();
        assert!(verify_credentials_on_snapshot(&pool, &[0x99; 32])
            .await
            .is_err());
        sqlx::query("UPDATE xcocs SET authorization_code_hash=?")
            .bind(vec![0u8; 32])
            .execute(&pool)
            .await
            .unwrap();
        assert!(verify_credentials_on_snapshot(&pool, &key).await.is_err());
        pool.close().await;
    }

    fn private_directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700)).unwrap();
        }
        directory
    }

    #[test]
    fn relocated_template_generates_matching_callback_and_storage() {
        let temporary = private_directory();
        let copied = temporary.path().join("arbitrary-name.yml");
        fs::write(&copied, MANAGED_MEDIA_CONFIG).unwrap();
        for callback in ["127.0.0.1:18107", "127.0.0.2:19107", "[::1]:19107"] {
            let callback = callback.parse().unwrap();
            let generated = render_media_config(&copied, temporary.path(), callback).unwrap();
            assert!(generated.contains(&format!(
                "authHTTPAddress: http://{callback}{}",
                CONTRACT.media_auth_path
            )));
            verify_media_config_content(&generated, temporary.path(), callback).unwrap();
        }
        assert!(
            render_media_config(&copied, temporary.path(), "0.0.0.0:18107".parse().unwrap())
                .is_err()
        );
    }

    #[test]
    fn external_yaml_record_path_cannot_be_overridden_or_escape_storage() {
        let temporary = private_directory();
        let path = format!("{}/%path/%Y-%m-%d", temporary.path().display());
        let content = MANAGED_MEDIA_CONFIG.replace(MANAGED_RECORD_PATH, &path);
        verify_media_config_content(
            &content,
            temporary.path(),
            "127.0.0.1:18107".parse().unwrap(),
        )
        .unwrap();
        let other = private_directory();
        assert!(verify_media_config_content(
            &content,
            other.path(),
            "127.0.0.1:18107".parse().unwrap()
        )
        .is_err());
        for value in [
            format!("{}/%path/%path", temporary.path().display()),
            format!("{}/%path/../escape", temporary.path().display()),
            format!("{}/%pathsuffix/%Y", temporary.path().display()),
            "relative/%path/%Y".into(),
        ] {
            assert!(verify_record_path(&value, temporary.path()).is_err());
        }
        assert!(verify_media_config_content(
            &format!("{content}\nrecordPath: {path}\n"),
            temporary.path(),
            "127.0.0.1:18107".parse().unwrap(),
        )
        .is_err());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn companion_version_probe_bounds_both_pipes_and_reaps_timed_out_process() {
        use std::os::unix::fs::PermissionsExt;
        let temporary = tempfile::tempdir().unwrap();
        let binary = temporary.path().join("companion");
        fs::write(
            &binary,
            "#!/bin/sh\nprintf 'v1.20.0\\n'\nprintf warning >&2\n",
        )
        .unwrap();
        fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
        assert_eq!(
            bounded_companion_version(&binary).await.unwrap(),
            b"v1.20.0\n"
        );
        for pipe in ["", " >&2"] {
            fs::write(
                &binary,
                format!(
                    "#!/bin/sh\nwhile :; do printf '{}'{pipe}; done\n",
                    "x".repeat(257)
                ),
            )
            .unwrap();
            let began = std::time::Instant::now();
            assert!(bounded_companion_version(&binary).await.is_err());
            assert!(began.elapsed() < Duration::from_secs(2));
        }
        let pid_file = temporary.path().join("pid");
        fs::write(
            &binary,
            format!(
                "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec sleep 30\n",
                pid_file.display()
            ),
        )
        .unwrap();
        let began = std::time::Instant::now();
        assert!(bounded_companion_version(&binary).await.is_err());
        assert!(began.elapsed() < Duration::from_secs(6));
        let pid = fs::read_to_string(pid_file).unwrap();
        assert!(!Path::new("/proc").join(pid).exists());
    }
}
