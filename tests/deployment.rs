use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};

#[test]
fn common_deployment_contract_owns_keys_and_keeps_configuration_unchanged() {
    let temporary = tempfile::tempdir().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = temporary.path();
    let binary = root.join("test-mediamtx");
    let script = b"#!/bin/sh\n[ \"$1\" = --version ] || exit 1\nprintf '%s\\n' v1.20.0\n";
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let contract = root.join("test-mediamtx.lock");
    fs::write(
        &contract,
        format!(
            "version=v1.20.0\nplatform=linux_amd64\nsha256={}\n",
            hex::encode(Sha256::digest(script))
        ),
    )
    .unwrap();
    let template = root.join("test-mediamtx.yml");
    fs::write(&template, include_str!("../config/mediamtx.yml")).unwrap();
    let data = root.join("fresh-data");
    let config = root.join("config.json");
    fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "data_dir": data,
            "public_rtsp_publish_base_url": "rtsps://example.test:8322",
            "mediamtx_config": template,
            "mediamtx_contract": contract,
            "mediamtx_binary": binary
        }))
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    xcss::testkit::assert_deployment_cli_contract(
        Path::new(env!("CARGO_BIN_EXE_xcos")),
        &config,
        &data,
        &["app.sqlite3", "secrets.json"],
    );
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let keys = fs::read(data.join("secrets.json")).unwrap();
    let reset = || {
        let mut child = Command::new(env!("CARGO_BIN_EXE_xcos"))
            .arg("--config")
            .arg(&config)
            .args(["admin", "reset-password", "--username", "admin"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(b"new independent administrator password\n")
            .unwrap();
        child.wait_with_output().unwrap()
    };
    let result = reset();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(keys, fs::read(data.join("secrets.json")).unwrap());
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let pool = sqlx::SqlitePool::connect(&format!("sqlite://{}", data.join("app.sqlite3").display())).await.unwrap();
        let (hash, version): (String, i64) = sqlx::query_as("SELECT password_hash, session_version FROM _common_administrators WHERE username='admin'").fetch_one(&pool).await.unwrap();
        assert!(xcss::admin_auth::verify_password("new independent administrator password", &hash));
        assert!(!xcss::admin_auth::verify_password("correct horse battery", &hash));
        assert_eq!(version, 2);
        pool.close().await;
    });
    let directory = xcss::state_file::PrivateStateDirectory::open(&data).unwrap();
    let _running = directory.try_instance_lock().unwrap();
    assert!(
        !reset().status.success(),
        "offline reset must reject running state"
    );
    assert_eq!(keys, fs::read(data.join("secrets.json")).unwrap());
}
