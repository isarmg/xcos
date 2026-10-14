//! Private state provisioning for the real-process test, not an installation API.
#[path = "administrator.rs"]
mod administrator;
#[path = "../../src/schema_definition.rs"]
mod schema_definition;
use base64::{engine::general_purpose::STANDARD, Engine as _};
use std::{fs, net::TcpListener, os::unix::fs::PermissionsExt, path::Path, process::Command};

pub fn private_write(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
pub fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}
pub async fn initialize(root: &Path, port: u16, companion: &Path) -> Vec<(String, String)> {
    for dir in [
        "config",
        "logs",
        "run",
        "state",
        "state/recordings",
        "bin",
        "share",
    ] {
        fs::create_dir(root.join(dir)).unwrap();
        fs::set_permissions(root.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
    }
    fs::copy(companion, root.join("bin/mediamtx")).unwrap();
    fs::set_permissions(root.join("bin/mediamtx"), fs::Permissions::from_mode(0o700)).unwrap();
    private_write(
        &root.join("share/mediamtx.lock"),
        include_str!("../../config/mediamtx.lock"),
    );
    let callback = free_port();
    let rtsps_port = free_port();
    private_write(&root.join("config/config.json"), serde_json::json!({
        "bind":format!("127.0.0.1:{port}"),"media_callback_bind":format!("127.0.0.1:{callback}"),
        "development":true,"public_rtsp_publish_base_url":format!("rtsps://127.0.0.1:{rtsps_port}")
    }).to_string());
    let media = include_str!("../../config/mediamtx.yml")
        .replace(
            "http://127.0.0.1:18107/internal",
            &format!("http://127.0.0.1:{callback}/internal"),
        )
        .replace(
            "/opt/isarmg/xcos/state/recordings",
            root.join("state/recordings").to_str().unwrap(),
        );
    private_write(&root.join("config/mediamtx.yml"), media);
    let certificate = root.join("config/rtsp-cert.pem");
    let key = root.join("config/rtsp-key.pem");
    assert!(Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "1",
            "-subj",
            "/CN=127.0.0.1",
            "-addext",
            "subjectAltName=IP:127.0.0.1",
            "-keyout"
        ])
        .arg(&key)
        .arg("-out")
        .arg(&certificate)
        .output()
        .unwrap()
        .status
        .success());
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let env = vec![
        ("MTX_WEBRTCADDITIONALHOSTS".into(), "127.0.0.1".into()),
        ("MTX_RTSPENCRYPTION".into(), "strict".into()),
        ("MTX_RTSPSADDRESS".into(), format!("127.0.0.1:{rtsps_port}")),
        (
            "MTX_RTSPSERVERCERT".into(),
            certificate.display().to_string(),
        ),
        ("MTX_RTSPSERVERKEY".into(), key.display().to_string()),
        ("MTX_PATHDEFAULTS_RECORDDELETEAFTER".into(), "168h".into()),
    ];
    private_write(
        &root.join("config/mediamtx.env"),
        env.iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<String>(),
    );
    let database = root.join("state/app.sqlite3");
    private_write(&database, []);
    let pool = xcss::sqlite::open_existing(&database, xcss::sqlite::PoolOptions::new(2))
        .await
        .unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::raw_sql(include_str!(concat!(
        env!("OUT_DIR"),
        "/current-schema.sql"
    )))
    .execute(&mut *tx)
    .await
    .unwrap();
    xcss::platform_db::initialize_current_platform_metadata(&mut tx, "server-control-plane", 1)
        .await
        .unwrap();
    sqlx::query(schema_definition::INITIAL_RECONCILER_LEASE_SQL)
        .execute(&mut *tx)
        .await
        .unwrap();
    let fingerprint = xcss::sqlite::schema_fingerprint(&mut *tx).await.unwrap();
    schema_definition::identity(env!("XCOS_CURRENT_SCHEMA_SHA256"))
        .unwrap()
        .verify_fingerprint(&fingerprint)
        .unwrap();
    sqlx::query(xcss::schema_identity::PRODUCT_METADATA_INSERT_SQL)
        .bind(schema_definition::APPLICATION)
        .bind(schema_definition::APPLICATION_VERSION)
        .bind(schema_definition::SCHEMA_REVISION)
        .bind(fingerprint)
        .execute(&mut *tx)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    let service = xcss::admin_core::AdministratorService::new(
        xcss::admin_sqlite::SqliteAdministratorStore::new(pool.clone()),
    );
    administrator::seed_administrator(&service, "fixture-admin", "fixture-password-2026", 1)
        .await
        .unwrap();
    xcss::sqlite::checkpoint(&pool).await.unwrap();
    pool.close().await;
    xcss::server_secrets::initialize(&root.join("state"),&serde_json::json!({"format":1,"jwt_secret":STANDARD.encode([7;32]),"credentials_key":STANDARD.encode([9;32])})).unwrap();
    env
}
