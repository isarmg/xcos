//! Real candidate-process HTTP smoke with development:true on loopback.
//! Private fixture provisioning is not an installer or production-TLS test.
#[path = "support/runtime_fixture.rs"]
mod fixture;

use serde_json::{json, Value};
use std::{
    fs,
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct Server(Child);
impl Server {
    fn start(root: &Path, base: &str) -> Self {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(root.join("logs/smoke.log"))
            .unwrap();
        let child = Command::new(env!("CARGO_BIN_EXE_xcos"))
            .env_clear()
            .arg("--root")
            .arg(root)
            .stdout(Stdio::from(log.try_clone().unwrap()))
            .stderr(Stdio::from(log))
            .spawn()
            .unwrap();
        let mut server = Self(child);
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            let ready = Command::new("curl")
                .args([
                    "--noproxy",
                    "*",
                    "--silent",
                    "--fail",
                    "--max-time",
                    "1",
                    &format!("{base}/readyz"),
                ])
                .output()
                .unwrap();
            if ready.status.success() {
                return server;
            }
            assert!(
                server.0.try_wait().unwrap().is_none(),
                "server exited: {}",
                fs::read_to_string(root.join("logs/smoke.log")).unwrap()
            );
            assert!(Instant::now() < deadline, "readiness timed out");
            thread::sleep(Duration::from_millis(100));
        }
    }
    fn stop(&mut self) {
        assert!(Command::new("kill")
            .args(["-TERM", &self.0.id().to_string()])
            .status()
            .unwrap()
            .success());
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(status) = self.0.try_wait().unwrap() {
                assert!(status.success(), "unclean server exit");
                return;
            }
            assert!(Instant::now() < deadline, "shutdown timed out");
            thread::sleep(Duration::from_millis(100));
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn request(
    root: &Path,
    base: &str,
    method: &str,
    path: &str,
    body: Option<Value>,
    csrf: Option<&str>,
) -> Value {
    let cookie = root.join("run/cookies");
    let mut command = Command::new("curl");
    command
        .args([
            "--noproxy",
            "*",
            "--silent",
            "--show-error",
            "--fail",
            "--max-time",
            "5",
            "-X",
            method,
            "-b",
        ])
        .arg(&cookie)
        .arg("-c")
        .arg(&cookie)
        .args([
            "-H",
            &format!("Origin: {base}"),
            "-H",
            "Sec-Fetch-Site: same-origin",
        ]);
    if let Some(csrf) = csrf {
        command.args(["-H", &format!("X-CSRF-Token: {csrf}")]);
    }
    if let Some(body) = body {
        command.args([
            "-H",
            "Content-Type: application/json",
            "--data-binary",
            &body.to_string(),
        ]);
    }
    let result = command.arg(format!("{base}{path}")).output().unwrap();
    assert!(
        result.status.success(),
        "{method} {path} failed: {}",
        String::from_utf8_lossy(&result.stderr)
    );
    serde_json::from_slice(&result.stdout).unwrap()
}
#[tokio::test]
#[ignore = "requires the SHA256-pinned real MediaMTX artifact; CI invokes explicitly"]
async fn real_companion_and_control_plane_preserve_instance_changes() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    let companion = std::path::PathBuf::from(
        std::env::var_os("XCOS_TEST_MEDIAMTX").expect("explicit pinned companion required"),
    );
    // These API/playback ports are part of the fixed application contract.
    // Refuse occupied ports; never connect to another local installation.
    let api_reservation = TcpListener::bind("127.0.0.1:9997").unwrap();
    let playback_reservation = TcpListener::bind("127.0.0.1:9996").unwrap();
    let port = fixture::free_port();
    let base = format!("http://127.0.0.1:{port}");
    let media_env = fixture::initialize(root, port, &companion).await;
    let log = fs::File::create(root.join("logs/companion.log")).unwrap();
    let mut command = Command::new(root.join("bin/mediamtx"));
    command
        .arg(root.join("config/mediamtx.yml"))
        .current_dir(root)
        .env_clear()
        .envs(media_env)
        .stdout(Stdio::from(log.try_clone().unwrap()))
        .stderr(Stdio::from(log));
    for name in ["RTSP", "METRICS", "HLS", "WEBRTC"] {
        command.env(
            format!("MTX_{name}ADDRESS"),
            format!("127.0.0.1:{}", fixture::free_port()),
        );
    }
    let udp = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
    command.env(
        "MTX_WEBRTCLOCALUDPADDRESS",
        udp.local_addr().unwrap().to_string(),
    );
    drop(udp);
    drop(api_reservation);
    drop(playback_reservation);
    let mut media = Server(command.spawn().unwrap());
    let mut server = Server::start(root, &base);
    let session = request(
        root,
        &base,
        "POST",
        "/api/v1/auth/login",
        Some(json!({"username":"fixture-admin", "password":"fixture-password-2026"})),
        None,
    );
    assert_eq!(session["authenticated"], true);
    let csrf = session["csrf_token"].as_str().unwrap();
    let created = request(
        root,
        &base,
        "POST",
        "/api/v1/clients",
        Some(json!({"name":"Original smoke instance"})),
        Some(csrf),
    );
    let id = created["id"].as_str().unwrap();
    let path = format!("/api/v1/clients/{id}");
    request(
        root,
        &base,
        "PATCH",
        &path,
        Some(json!({"name":"Renamed smoke instance"})),
        Some(csrf),
    );
    let code = "r".repeat(36);
    request(
        root,
        &base,
        "PUT",
        &format!("{path}/authorization"),
        Some(json!({"authorization_code":code})),
        Some(csrf),
    );
    let before = request(root, &base, "GET", "/api/v1/clients", None, None);
    assert_eq!(before.as_array().unwrap().len(), 1);
    assert_eq!(before[0]["id"], id);
    assert_eq!(before[0]["name"], "Renamed smoke instance");
    assert_eq!(before[0]["authorization_code"], code);
    server.stop();
    let mut server = Server::start(root, &base);
    // Reuse the actual cookie, without re-seeding the database or logging in again.
    let persisted = request(root, &base, "GET", "/api/v1/auth/session", None, None);
    assert_eq!(persisted["authenticated"], true);
    assert_eq!(persisted["user_id"], session["user_id"]);
    let after = request(root, &base, "GET", "/api/v1/clients", None, None);
    assert_eq!(after, before);
    server.stop();
    media.stop();
}
