use std::{fs, os::unix::fs::PermissionsExt, process::Command};

#[test]
fn runtime_rejects_missing_state_without_initializing_it() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join("config")).unwrap();
    let config = root.path().join("config/config.json");
    fs::write(
        &config,
        br#"{"public_rtsp_publish_base_url":"rtsps://camera.example.org:8322"}"#,
    )
    .unwrap();
    fs::set_permissions(&config, fs::Permissions::from_mode(0o600)).unwrap();
    let original = fs::read(&config).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_xcos"))
        .env_clear()
        .arg("--root")
        .arg(root.path())
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!root.path().join("state").exists());
    assert!(!root.path().join("logs").exists());
    assert!(!root.path().join("run").exists());
    assert_eq!(fs::read(&config).unwrap(), original);
}

#[test]
fn runtime_has_no_installation_or_offline_maintenance_entrypoints() {
    let root = tempfile::tempdir().unwrap();
    for command in [
        "init",
        "check",
        "config",
        "admin",
        "reset-password",
        "status",
        "web-assets",
        "state-contract",
        "release-identity",
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_xcos"))
            .env_clear()
            .arg("--root")
            .arg(root.path())
            .arg(command)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "accepted removed command {command}"
        );
    }
    assert_eq!(fs::read_dir(root.path()).unwrap().count(), 0);
    assert!(!Command::new(env!("CARGO_BIN_EXE_xcos"))
        .env_clear()
        .output()
        .unwrap()
        .status
        .success());
}
