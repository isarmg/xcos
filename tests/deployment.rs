use sha2::{Digest, Sha256};
use std::{fs, os::unix::fs::PermissionsExt};

#[tokio::test]
async fn offline_setup_owns_initialization_and_reset_preserves_instance_keys() {
    let temporary = tempfile::tempdir().unwrap();
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root_path = temporary.path().join("root with spaces");
    fs::create_dir(&root_path).unwrap();
    fs::set_permissions(&root_path, fs::Permissions::from_mode(0o700)).unwrap();
    let root = root_path.as_path();
    for child in ["bin", "config", "share"] {
        fs::create_dir(root.join(child)).unwrap();
        fs::set_permissions(root.join(child), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let binary = root.join("bin/mediamtx");
    let script = b"#!/bin/sh\n[ \"$1\" = --version ] || exit 1\nprintf '%s\\n' v1.20.0\n";
    fs::write(&binary, script).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    let contract = root.join("share/mediamtx.lock");
    fs::write(
        &contract,
        format!(
            "version=v1.20.0\nplatform=linux_amd64\nsha256={}\n",
            hex::encode(Sha256::digest(script))
        ),
    )
    .unwrap();
    fs::write(
        root.join("share/mediamtx.yml"),
        include_str!("../config/mediamtx.yml"),
    )
    .unwrap();
    let data = root.join("state");
    let config = root.join("config/config.json");
    fs::write(
        &config,
        serde_json::to_vec(&serde_json::json!({
            "data_dir": data,
            "public_rtsp_publish_base_url": "rtsps://example.test:8322",
            "mediamtx_config": root.join("config/mediamtx.yml"),
            "mediamtx_contract": contract,
            "mediamtx_binary": binary
        }))
        .unwrap(),
    )
    .unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::read(&config).unwrap();
    xcos::offline::validate_config(root, &config).await.unwrap();
    assert!(!data.exists(), "validation must not initialize state");
    xcos::offline::initialize(root, &config, "admin", "correct horse battery")
        .await
        .unwrap();
    xcos::offline::check(root, &config).await.unwrap();
    assert!(data.join("recordings").is_dir());
    let environment = root.join("config/mediamtx.env");
    let original_environment = fs::read(&environment).unwrap();
    let text = std::str::from_utf8(&original_environment).unwrap();
    assert!(text.contains("MTX_RTSPENCRYPTION=\"strict\""));
    assert!(text.contains(root.join("config/rtsp-cert.pem").to_str().unwrap()));
    fs::write(
        &environment,
        format!("{text}MTX_AUTHHTTPADDRESS=\"http://invalid.test/\"\n"),
    )
    .unwrap();
    assert!(xcos::offline::check(root, &config).await.is_err());
    for retention in ["0s", "7d", "1h30m"] {
        fs::write(&environment, text.replace("168h", retention)).unwrap();
        xcos::offline::check(root, &config).await.unwrap();
    }
    fs::write(&environment, text.replace("168h", "1s")).unwrap();
    assert!(xcos::offline::check(root, &config).await.is_err());
    fs::write(&environment, &original_environment).unwrap();
    xcos::offline::check(root, &config).await.unwrap();
    assert!(root.join("logs").is_dir());
    assert!(root.join("run").is_dir());
    assert_eq!(before, fs::read(&config).unwrap());
    let keys = fs::read(data.join("secrets.json")).unwrap();
    assert!(
        xcos::offline::initialize(root, &config, "admin", "another strong password")
            .await
            .is_err()
    );
    assert_eq!(keys, fs::read(data.join("secrets.json")).unwrap());
    xcos::offline::reset_password(
        root,
        &config,
        "admin",
        "new independent administrator password",
    )
    .await
    .unwrap();
    assert_eq!(keys, fs::read(data.join("secrets.json")).unwrap());
    let pool =
        sqlx::SqlitePool::connect(&format!("sqlite://{}", data.join("app.sqlite3").display()))
            .await
            .unwrap();
    let (hash, version): (String, i64) = sqlx::query_as(
        "SELECT password_hash, session_version FROM _common_administrators WHERE username='admin'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(xcss::admin_auth::verify_password(
        "new independent administrator password",
        &hash
    ));
    assert!(!xcss::admin_auth::verify_password(
        "correct horse battery",
        &hash
    ));
    assert_eq!(version, 2);
    pool.close().await;
    let directory = xcss::state_file::PrivateStateDirectory::open(&data).unwrap();
    let _running = directory.try_instance_lock().unwrap();
    assert!(xcos::offline::reset_password(
        root,
        &config,
        "admin",
        "different independent administrator password"
    )
    .await
    .is_err());
    assert_eq!(keys, fs::read(data.join("secrets.json")).unwrap());
    assert!(xcos::offline::regenerate_media_config(root, &config)
        .await
        .is_err());
    assert_eq!(original_environment, fs::read(environment).unwrap());
    drop(_running);
    let pool =
        sqlx::SqlitePool::connect(&format!("sqlite://{}", data.join("app.sqlite3").display()))
            .await
            .unwrap();
    sqlx::query("UPDATE _common_administrators SET password_hash='invalid'")
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    let invalid_database = fs::read(data.join("app.sqlite3")).unwrap();
    for _ in 0..3 {
        assert!(xcos::offline::check(root, &config).await.is_err());
        let rejected = std::process::Command::new(env!("CARGO_BIN_EXE_xcos"))
            .args(["--root"])
            .arg(root)
            .output()
            .unwrap();
        assert!(
            !rejected.status.success(),
            "runtime accepted an invalid administrator hash"
        );
        assert_eq!(
            invalid_database,
            fs::read(data.join("app.sqlite3")).unwrap()
        );
        assert!(
            fs::read_dir(root.join("run")).unwrap().next().is_none(),
            "failed validation left a SQLite snapshot behind"
        );
    }
}
