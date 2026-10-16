use serde::{Deserialize, Serialize};
use serde_json::Value;
use xcss_web::{
    api::{self, ApiError},
    i18n::t,
};

macro_rules! string_enum {
    ($name:ident, $($choice:literal),+ $(,)?) => {
        fn $name<'de,D:serde::Deserializer<'de>>(deserializer:D)->Result<String,D::Error>{
            let value=String::deserialize(deserializer)?;
            if matches!(value.as_str(),$($choice)|+){Ok(value)}else{Err(serde::de::Error::custom("invalid protocol enum"))}
        }
    }
}
string_enum!(support, "supported", "unsupported", "unknown");
string_enum!(source_kind, "client");
string_enum!(storage_mode, "client", "server");
string_enum!(stream_profile, "main", "sub");
string_enum!(observation, "fresh", "stale", "unknown");
string_enum!(client_status, "pending", "online", "offline", "revoked");
string_enum!(
    receipt_state,
    "queued",
    "awaiting_result",
    "succeeded",
    "failed",
    "expired",
    "unconfirmed"
);
string_enum!(severity, "info", "warning", "critical");
fn required_nullable<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(
    d: D,
) -> Result<Option<T>, D::Error> {
    Option::deserialize(d)
}
fn authorization_code<'de, D: serde::Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    let value = String::deserialize(d)?;
    if value.len() == 36
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
    {
        Ok(value)
    } else {
        Err(serde::de::Error::custom("invalid authorization code"))
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Camera {
    pub id: String,
    pub name: String,
    pub location: String,
    pub has_sub_stream: bool,
    #[serde(deserialize_with = "source_kind")]
    pub source_kind: String,
    #[serde(deserialize_with = "required_nullable")]
    pub client_id: Option<String>,
    pub adapter_kind: String,
    #[serde(deserialize_with = "required_nullable")]
    pub manufacturer: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub model: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub firmware_version: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub serial_number: Option<String>,
    pub capabilities: Capabilities,
    pub streams: Vec<CameraStream>,
    #[serde(deserialize_with = "required_nullable")]
    pub health_message: Option<String>,
    pub device_status: String,
    #[serde(deserialize_with = "storage_mode")]
    pub storage_mode: String,
    pub enabled: bool,
    pub record_enabled: bool,
    pub status: String,
    #[serde(deserialize_with = "required_nullable")]
    pub last_seen_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    #[serde(deserialize_with = "observation")]
    pub observation_status: String,
    #[serde(deserialize_with = "required_nullable")]
    pub last_observed_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub observation_expires_at: Option<String>,
    pub observation_remaining_ms: u64,
    #[serde(skip)]
    pub observation_deadline: f64,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Capabilities {
    #[serde(deserialize_with = "support")]
    pub video: String,
    #[serde(deserialize_with = "support")]
    pub main_stream: String,
    #[serde(deserialize_with = "support")]
    pub sub_stream: String,
    #[serde(deserialize_with = "support")]
    pub local_recording: String,
    #[serde(deserialize_with = "support")]
    pub server_recording: String,
    #[serde(deserialize_with = "support")]
    pub ptz: String,
    #[serde(deserialize_with = "support")]
    pub events: String,
    #[serde(deserialize_with = "support")]
    pub audio_input: String,
    #[serde(deserialize_with = "support")]
    pub audio_output: String,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct CameraStream {
    #[serde(deserialize_with = "stream_profile")]
    pub profile: String,
    #[serde(deserialize_with = "required_nullable")]
    pub video_codec: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub audio_codec: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub width: Option<f64>,
    #[serde(deserialize_with = "required_nullable")]
    pub height: Option<f64>,
    #[serde(deserialize_with = "required_nullable")]
    pub frame_rate: Option<f64>,
}
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct Client {
    pub id: String,
    #[serde(deserialize_with = "required_nullable")]
    pub installation_id: Option<String>,
    pub name: String,
    #[serde(deserialize_with = "required_nullable")]
    pub client_version: Option<String>,
    #[serde(deserialize_with = "authorization_code")]
    pub authorization_code: String,
    #[serde(deserialize_with = "client_status")]
    pub status: String,
    #[serde(deserialize_with = "required_nullable")]
    pub last_seen_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)] // Retained fields validate the complete wire contract.
pub struct System {
    pub service: String,
    pub version: String,
    pub database: String,
    pub server_time: String,
    pub media_service: String,
    pub cameras: SystemCameras,
}
#[derive(Clone, Debug, Deserialize)]
pub struct SystemCameras {
    pub recording_configured: u64,
}
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)] // Retained fields validate the complete wire contract.
pub struct Ticket {
    pub profile: String,
    pub expires_at: String,
    pub whep_url: String,
    pub hls_url: String,
    pub token: String,
}
#[derive(Clone, Debug, Deserialize, PartialEq)]
pub struct Span {
    pub start: String,
    pub duration: f64,
}
#[derive(Clone, Debug, Deserialize)]
pub struct LogPage {
    pub format: String,
    pub items: Vec<Value>,
    #[serde(deserialize_with = "required_nullable")]
    pub next_cursor: Option<String>,
}
#[derive(Clone, Debug, Deserialize)]
#[allow(dead_code)] // Retained fields validate the complete wire contract.
pub struct Receipt {
    pub command_id: String,
    #[serde(deserialize_with = "receipt_state")]
    pub state: String,
    pub error_code: Option<String>,
}

/// The full receipt is required to prove a reconciliation result.
#[allow(dead_code)]
#[derive(Deserialize)]
pub struct Operation {
    pub id: String,
    pub camera_id: String,
    pub generation: f64,
    pub kind: String,
    pub state: String,
    pub reason: String,
    #[serde(deserialize_with = "required_nullable")]
    pub requested_by: Option<String>,
    pub attempt: f64,
    pub max_attempts: f64,
    pub created_at: String,
    #[serde(deserialize_with = "required_nullable")]
    pub started_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub finished_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub retry_at: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub error_code: Option<String>,
    #[serde(deserialize_with = "required_nullable")]
    pub error_message: Option<String>,
}

#[allow(dead_code)]
#[derive(Deserialize)]
struct EventRow {
    id: String,
    #[serde(deserialize_with = "required_nullable")]
    camera_id: Option<String>,
    kind: String,
    #[serde(deserialize_with = "severity")]
    severity: String,
    message: String,
    #[serde(deserialize_with = "required_nullable")]
    acknowledged_at: Option<String>,
    created_at: String,
}
#[allow(dead_code)]
#[derive(Deserialize)]
struct AuditRow {
    id: String,
    #[serde(deserialize_with = "required_nullable")]
    user_id: Option<String>,
    action: String,
    entity_type: String,
    #[serde(deserialize_with = "required_nullable")]
    entity_id: Option<String>,
    details: serde_json::Map<String, Value>,
    created_at: String,
}
pub fn valid_log_row(value: &Value, index: usize) -> bool {
    match index {
        0 => serde_json::from_value::<EventRow>(value.clone()).is_ok(),
        1 => serde_json::from_value::<Operation>(value.clone()).is_ok(),
        _ => serde_json::from_value::<AuditRow>(value.clone()).is_ok(),
    }
}

pub async fn request<T: serde::de::DeserializeOwned>(
    path: &str,
    method: &str,
    body: Option<Value>,
) -> Result<T, ApiError> {
    let value = api::request(&format!("/api/v1{path}"), method, body).await?;
    serde_json::from_value(value).map_err(|_| contract_error())
}
pub async fn command(path: &str, method: &str, body: Option<Value>) -> Result<(), ApiError> {
    let value = api::request(&format!("/api/v1{path}"), method, body).await?;
    if path.starts_with("/clients") && method != "DELETE" {
        serde_json::from_value::<Client>(value)
            .map(|_| ())
            .map_err(|_| contract_error())
    } else if path.starts_with("/media/operations/") && path.ends_with("/resolve") {
        serde_json::from_value::<Operation>(value)
            .map(|_| ())
            .map_err(|_| contract_error())
    } else if value.is_null() {
        Ok(())
    } else {
        Err(contract_error())
    }
}
pub fn contract_error() -> ApiError {
    ApiError {
        status: 200,
        code: "platform.invalid_response".into(),
        message: t(
            "服务器响应格式无效，无法确认操作结果。",
            "The server response was invalid. The operation result could not be confirmed.",
        ),
        request_id: None,
        retryable: false,
        retry_after_seconds: None,
    }
}
pub fn mono() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.)
}
pub fn status(camera: &Camera, now: f64) -> &str {
    if !camera.enabled {
        "disabled"
    } else if camera.observation_status != "fresh" {
        &camera.observation_status
    } else if camera.observation_expires_at.is_none() || camera.observation_deadline <= now {
        "stale"
    } else if camera.device_status == "online" {
        &camera.status
    } else {
        &camera.device_status
    }
}
pub fn valid_observation(c: &Camera) -> bool {
    if c.observation_remaining_ms > 9_007_199_254_740_991 {
        return false;
    }
    let date = |s: &str| js_sys::Date::parse(s).is_finite();
    match c.last_observed_at.as_ref() {
        None => {
            c.observation_status == "unknown"
                && c.observation_expires_at.is_none()
                && c.observation_remaining_ms == 0
        }
        Some(at) => {
            date(at)
                && c.observation_expires_at
                    .as_ref()
                    .is_none_or(|v| date(v) && js_sys::Date::parse(v) > js_sys::Date::parse(at))
                && ((c.observation_status == "stale" && c.observation_remaining_ms == 0)
                    || (c.observation_status == "fresh" && c.observation_expires_at.is_some()))
        }
    }
}
pub fn status_label(s: &str) -> String {
    match s {
        "pending" => t("等待检测", "Waiting for detection"),
        "online" => t("在线", "Online"),
        "offline" => t("离线", "Offline"),
        "disabled" => t("已停用", "Disabled"),
        "error" => t("设备异常", "Device error"),
        "stale" => t("观测已过期", "Observation stale"),
        "unknown" => t("尚无有效观测", "No valid observation"),
        _ => t("未知", "Unknown"),
    }
}
pub fn label(s: &str) -> String {
    match s {
        "camera.online" => t("摄像头已恢复在线", "Camera is back online"),
        "camera.offline" => t("摄像头已离线", "Camera is offline"),
        "camera.status" => t("摄像头状态变更", "Camera status changed"),
        "camera.updated" => t("更新摄像头", "Update camera"),
        "camera.ptz.queued" => t("云台命令已排队", "PTZ command queued"),
        "client.create" => t("创建实例", "Create instance"),
        "client.pair" => t("配对实例", "Pair instance"),
        "client.authorization.rotate" => t("更换密码", "Change password"),
        "client.revoke" => t("撤销实例", "Revoke instance"),
        "client.delete" => t("删除实例", "Delete instance"),
        "event.acknowledge" | "event.ack" => t("确认事件", "Acknowledge event"),
        "media.operation.resolve" => t("核对媒体操作", "Reconcile media operation"),
        "pending" => t("待配对", "Awaiting pairing"),
        "online" => t("在线", "Online"),
        "offline" => t("离线", "Offline"),
        "revoked" => t("已撤销", "Revoked"),
        "camera" => t("摄像头", "Camera"),
        "event" => t("事件", "Event"),
        "operation" => t("操作", "Operation"),
        _ => t("未识别的事件", "Unrecognized event"),
    }
}
pub fn value(v: &Value, k: &str) -> String {
    v.get(k).and_then(Value::as_str).unwrap_or("—").into()
}
pub fn date(v: &str) -> String {
    let d = js_sys::Date::new(&v.into());
    if d.get_time().is_finite() {
        format!(
            "{} UTC",
            d.to_iso_string()
                .as_string()
                .unwrap_or_default()
                .replace('T', " ")
                .replace(".000Z", "")
        )
    } else {
        t("未知", "Unknown")
    }
}
pub fn duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    let mut out = vec![];
    if s >= 3600 {
        out.push(format!("{}{}", s / 3600, t("时", "h")))
    }
    if s % 3600 >= 60 {
        out.push(format!("{}{}", s % 3600 / 60, t("分", "m")))
    }
    out.push(format!("{}{}", s % 60, t("秒", "s")));
    out.join(if xcss_web::i18n::locale() == "en" {
        " "
    } else {
        ""
    })
}
pub fn encode(v: &str) -> String {
    js_sys::encode_uri_component(v)
        .as_string()
        .unwrap_or_default()
}
pub fn valid_name(s: &str) -> bool {
    let n = s.trim_matches(char::is_whitespace);
    !n.is_empty() && n.chars().count() <= 64 && !n.chars().any(char::is_control)
}
pub fn random_code() -> String {
    let alphabet = b"abcdefghijklmnopqrstuvwxyz0123456789";
    let mut result = String::new();
    while result.len() < 36 {
        let mut bytes = [0u8; 64];
        web_sys::window()
            .unwrap()
            .crypto()
            .unwrap()
            .get_random_values_with_u8_array(&mut bytes)
            .unwrap();
        for b in bytes {
            if b < 252 {
                result.push(alphabet[b as usize % 36] as char);
                if result.len() == 36 {
                    break;
                }
            }
        }
    }
    result
}
pub fn safe_details(v: &Value) -> String {
    let mut copy = v.clone();
    if let Some(m) = copy.as_object_mut() {
        m.retain(|key, _| {
            let k = key.to_lowercase();
            !["password", "secret", "token", "authorization", "credential"]
                .iter()
                .any(|s| k.contains(s))
        });
    }
    copy.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_follow_unicode_scalar_limit() {
        assert!(valid_name("摄像头"));
        assert!(!valid_name(&"相".repeat(65)));
        assert!(!valid_name("a\u{7f}"));
        assert!(!valid_name("  "));
    }
    #[test]
    fn audit_secrets_are_removed() {
        assert_eq!(
            safe_details(&serde_json::json!({"Authorization":"secret","generation":2})),
            "{\"generation\":2}"
        );
    }
}

#[cfg(test)]
mod protocol_regressions {
    use super::*;
    fn camera() -> Camera {
        serde_json::from_value(serde_json::json!({"id":"camera","name":"门口","location":"","has_sub_stream":false,"source_kind":"client","client_id":"client","adapter_kind":"onvif","manufacturer":null,"model":null,"firmware_version":null,"serial_number":null,"capabilities":{"video":"supported","main_stream":"supported","sub_stream":"unsupported","local_recording":"unknown","server_recording":"supported","ptz":"supported","events":"unknown","audio_input":"unknown","audio_output":"unsupported"},"streams":[],"health_message":null,"device_status":"online","storage_mode":"server","enabled":true,"record_enabled":true,"status":"online","last_seen_at":null,"created_at":"2026-10-16T00:00:00Z","updated_at":"2026-10-16T00:00:00Z","observation_status":"fresh","last_observed_at":"2026-10-16T00:00:00Z","observation_expires_at":"2026-10-16T00:01:00Z","observation_remaining_ms":60_000})).unwrap()
    }
    #[test]
    fn observation_deadline_is_monotonic_and_cannot_be_extended_by_wall_clock_jumps() {
        let mut c = camera();
        c.observation_deadline = 61_000.;
        assert_eq!(status(&c, 1_000.), "online");
        assert_eq!(status(&c, 60_999.), "online");
        assert_eq!(status(&c, 61_000.), "stale");
        c.enabled = false;
        assert_eq!(status(&c, 1_000.), "disabled");
    }
    #[test]
    fn missing_nullable_fields_and_unknown_command_states_are_contract_failures() {
        let mut value = serde_json::to_value(camera()).unwrap();
        value.as_object_mut().unwrap().remove("last_seen_at");
        assert!(serde_json::from_value::<Camera>(value).is_err());
        assert!(
            serde_json::from_value::<Receipt>(
                serde_json::json!({"command_id":"command","state":"done"})
            )
            .is_err()
        );
    }
    #[test]
    fn product_and_administrator_api_prefix_match() {
        let contract: Value =
            serde_json::from_str(include_str!("../../src/protocol-contract.json")).unwrap();
        let prefix = contract["api_prefix"].as_str().unwrap();
        assert_eq!(
            format!("{prefix}/auth/login"),
            xcss_web::contracts::LOGIN_PATH
        );
    }
}
