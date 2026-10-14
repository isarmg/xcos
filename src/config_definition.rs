//! Current configuration data definition shared by runtime parsing and build-time contract generation.
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

pub(crate) const DEFAULT_BIND_ADDR: &str = "127.0.0.1:18106";
pub(crate) const MEDIA_API_ADDRESS: &str = "127.0.0.1:9997";
pub(crate) const MEDIA_PLAYBACK_ADDRESS: &str = "127.0.0.1:9996";
pub(crate) const MEDIA_API_ORIGIN: &str = "http://127.0.0.1:9997";
pub(crate) const MEDIA_PLAYBACK_ORIGIN: &str = "http://127.0.0.1:9996";

#[derive(Clone, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    #[serde(skip)]
    pub root: PathBuf,
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
    pub public_webrtc_base_url: String,
    pub public_hls_base_url: String,
    pub public_rtsp_publish_base_url: String,
    pub status_interval_secs: u64,
    pub reconcile_interval_secs: u64,
    pub request_timeout_secs: u64,
    pub static_dir: Option<PathBuf>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            root: "/opt/isarmg/xcos".into(),
            recordings_directory: None,
            mediamtx_config: Some("/opt/isarmg/xcos/config/mediamtx.yml".into()),
            mediamtx_contract: Some("/opt/isarmg/xcos/share/mediamtx.lock".into()),
            mediamtx_binary: Some("/opt/isarmg/xcos/bin/mediamtx".into()),
            bind: DEFAULT_BIND_ADDR.parse().expect("default address"),
            media_callback_bind: "127.0.0.1:18107".parse().expect("callback address"),
            data_dir: Some("/opt/isarmg/xcos/state".into()),
            development: false,
            log_level: "info".into(),
            media_token_ttl_secs: 120,
            public_webrtc_base_url: "/media-webrtc".into(),
            public_hls_base_url: "/media-hls".into(),
            public_rtsp_publish_base_url: "rtsp://127.0.0.1:8554".into(),
            status_interval_secs: 10,
            reconcile_interval_secs: 60,
            request_timeout_secs: 20,
            static_dir: None,
        }
    }
}

pub(crate) fn data_path_spec() -> serde_json::Value {
    serde_json::json!({"base":"config","value":"/data_dir","fallback":null,"suffix":"","encoding":"path"})
}
pub(crate) fn recordings_path_spec() -> serde_json::Value {
    let mut fallback = data_path_spec();
    fallback["suffix"] = serde_json::json!("recordings");
    serde_json::json!({"base":"config","value":"/recordings_directory","fallback":fallback,"suffix":"","encoding":"path"})
}
pub(crate) fn fixed_companion_paths() -> [(&'static str, &'static str); 3] {
    [
        ("/mediamtx_config", "config/mediamtx.yml"),
        ("/mediamtx_contract", "share/mediamtx.lock"),
        ("/mediamtx_binary", "bin/mediamtx"),
    ]
}
pub(crate) fn current_rules(final_configuration: bool, source_bound: bool) -> serde_json::Value {
    use serde_json::json;
    let mut rules = vec![
        json!({"kind":"enum","fields":["/log_level"],"arguments":{"values":["off","error","warn","info","debug","trace"]}}),
        json!({"kind":"socket-address","fields":["/bind"],"arguments":{"loopback_when":"/development"}}),
        json!({"kind":"socket-address","fields":["/media_callback_bind"],"arguments":{"loopback":true,"nonzero":true}}),
        json!({"kind":"distinct-sockets","fields":["/bind","/media_callback_bind"],"arguments":{}}),
        json!({"kind":"external-web-override","fields":["/static_dir"],"arguments":{"development_pointer":"/development","source_bound":source_bound}}),
        json!({"kind":"root-state-paths","fields":[],"arguments":{"paths":[data_path_spec()],"protected":xcss::server_layout::ROOT_RESERVED_DIRECTORIES}}),
        json!({"kind":"root-state-paths","fields":[],"arguments":{"paths":[recordings_path_spec()],"protected":xcss::server_layout::ROOT_RESERVED_DIRECTORIES}}),
        json!({"kind":"path-relation","fields":[],"arguments":{"parent":data_path_spec(),"child":recordings_path_spec(),"allow_child_subtree":true,"forbidden_child_first_component_prefixes":["app.sqlite3","secrets.json",".state-"]}}),
    ];
    for (field, optional) in [
        ("/data_dir", false),
        ("/recordings_directory", true),
        ("/static_dir", true),
        ("/mediamtx_config", true),
        ("/mediamtx_contract", true),
        ("/mediamtx_binary", true),
    ] {
        rules.push(json!({"kind":"path","fields":[field],"arguments":{"absolute":true,"optional":optional}}));
    }
    for (field, minimum, maximum) in [
        ("/media_token_ttl_secs", 30, 300),
        ("/status_interval_secs", 1, 3600),
        ("/reconcile_interval_secs", 1, 3600),
        ("/request_timeout_secs", 1, 300),
    ] {
        rules.push(json!({"kind":"integer-range","fields":[field],"arguments":{"minimum":minimum,"maximum":maximum}}));
    }
    rules.push(json!({"kind":"url","fields":["/public_rtsp_publish_base_url"],"arguments":{"allowed_schemes":["rtsp","rtsps"],"no_credentials":true,"no_query":true,"no_fragment":true,"origin_only":true,
        "production_public_rtsps":final_configuration,"development_pointer":"/development"}}));
    json!(rules)
}
