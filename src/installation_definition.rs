//! Export current product-owned definitions; independent tools perform maintenance.
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::path::Path;
use xcss::contracts::{ReleaseIdentity, SchemaIdentity};

fn sha(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn root(value: &str) -> Value {
    json!({"base":"root","value":value,"fallback":null,"suffix":"","encoding":"path"})
}
fn data(value: &str) -> Value {
    let mut path = crate::config_definition::data_path_spec();
    path["suffix"] = json!(value);
    path
}
fn source(kind: &str, value: &str, prefix: &str, suffix: &str) -> Value {
    json!({"kind":kind,"value":value,"prefix":prefix,"suffix":suffix})
}
fn asset(directory: &Path, name: &str, bytes: &[u8]) -> Value {
    std::fs::write(directory.join(name), bytes).unwrap();
    json!({"path":format!("share/contracts/{name}"),"sha256":sha(bytes)})
}
fn relocate(value: &mut Value) {
    match value {
        Value::String(text) => {
            if let Some(suffix) = text.strip_prefix("/opt/isarmg/xcos/") {
                *text = format!("$ROOT/{suffix}");
            }
        }
        Value::Object(items) => items.values_mut().for_each(relocate),
        Value::Array(items) => items.iter_mut().for_each(relocate),
        _ => {}
    }
}
fn media_template() -> String {
    include_str!("../config/mediamtx.yml")
        .lines()
        .map(|line| {
            let indentation = &line[..line.len() - line.trim_start().len()];
            if line.trim_start().starts_with("authHTTPAddress:") {
                format!("{indentation}authHTTPAddress: @@CALLBACK@@\n")
            } else if line.trim_start().starts_with("recordPath:") {
                format!("{indentation}recordPath: @@RECORD_PATH@@\n")
            } else {
                format!("{line}\n")
            }
        })
        .collect()
}
fn media(directory: &Path) -> Value {
    let template = media_template();
    let lock = include_str!("../config/mediamtx.lock");
    let field = |name| {
        lock.lines()
            .find_map(|line| line.strip_prefix(name))
            .expect("owned companion lock")
    };
    let hash = field("sha256=");
    let callback = source(
        "config",
        "/media_callback_bind",
        "http://",
        "/internal/v1/media/auth",
    );
    let records = source(
        "resource",
        "/resources/business_paths/0",
        "",
        "/%path/%Y-%m-%d_%H-%M-%S-%f",
    );
    let scalars = json!({"authMethod":"http","authHTTPAddress":callback,"apiAddress":crate::config_definition::MEDIA_API_ADDRESS,
        "playbackAddress":crate::config_definition::MEDIA_PLAYBACK_ADDRESS,"recordFormat":"fmp4","rtmp":"no","srt":"no","moq":"no"});
    let mut yaml_cases = vec![
        json!({"name":"current-managed-config","input":template,"valid":true}),
        json!({"name":"comments-do-not-change-current-config","input":format!("# current operator comment\n{template}"),"valid":true}),
    ];
    for (name, input) in [
        (
            "authentication-must-be-http",
            template.replace("authMethod: http", "authMethod: internal"),
        ),
        (
            "companion-port-must-match",
            template.replace("apiAddress: 127.0.0.1:9997", "apiAddress: 127.0.0.1:19997"),
        ),
        (
            "unexpected-auto-key-writer",
            template.replace("moq: no", "moq: yes"),
        ),
        (
            "duplicate-required-key",
            format!("{template}\nauthMethod: http\n"),
        ),
        (
            "different-recordings-root",
            template.replace("@@RECORD_PATH@@", "\"$ROOT/unknown/%path/%Y\""),
        ),
        (
            "record-path-traversal",
            template.replace(
                "@@RECORD_PATH@@",
                "\"$ROOT/state/recordings/%path/../escape\"",
            ),
        ),
    ] {
        yaml_cases.push(json!({"name":name,"input":input,"valid":false}));
    }
    let env = "MTX_WEBRTCADDITIONALHOSTS=camera.example.org\nMTX_RTSPENCRYPTION=strict\nMTX_RTSPSADDRESS=:8322\nMTX_RTSPSERVERCERT=\"$ROOT/config/rtsp-cert.pem\"\nMTX_RTSPSERVERKEY=\"$ROOT/config/rtsp-key.pem\"\nMTX_PATHDEFAULTS_RECORDDELETEAFTER=168h\n";
    let mut environment_cases = vec![
        json!({"name":"current-environment","input":env,"valid":true}),
        json!({"name":"current-zero-retention","input":env.replace("168h","0"),"valid":true}),
        json!({"name":"current-day-retention","input":env.replace("168h","7d"),"valid":true}),
    ];
    environment_cases.push(json!({"name":"native-positive-duration-prefix","input":env.replace("168h","+168h"),"valid":true}));
    environment_cases.push(json!({"name":"dns-host-case-insensitive","input":env.replace("camera.example.org","Camera.Example.ORG"),"valid":true}));
    for (name, input) in [
        ("unknown-environment-key", format!("{env}MTX_REMOVED=1\n")),
        (
            "duplicate-environment-key",
            format!("{env}MTX_RTSPENCRYPTION=strict\n"),
        ),
        ("tls-required", env.replace("strict", "no")),
        (
            "fixed-certificate-path",
            env.replace("$ROOT/config/rtsp-cert.pem", "$ROOT/config/unknown.pem"),
        ),
        ("zero-port", env.replace(":8322", ":0")),
        (
            "ice-host-with-path",
            env.replace("camera.example.org", "camera.example.org/path"),
        ),
        ("negative-retention", env.replace("168h", "-1h")),
        ("retention-below-segment", env.replace("168h", "1s")),
        (
            "retention-overflow",
            env.replace("168h", "999999999999999999999h"),
        ),
        ("invalid-day-position", env.replace("168h", "1h1d")),
    ] {
        environment_cases.push(json!({"name":name,"input":input,"valid":false}));
    }
    let certificate = source("root", "", "", "/config/rtsp-cert.pem");
    let key = source("root", "", "", "/config/rtsp-key.pem");
    let environment = json!({"kind":"environment-v1","description":"Current companion TLS, ICE and native duration contract",
        "parameters":{"env_path":root("config/mediamtx.env"),"maximum_bytes":16384,
            "keys":{"MTX_WEBRTCADDITIONALHOSTS":{"kind":"host-list","case_insensitive":true},"MTX_RTSPENCRYPTION":{"kind":"literal","value":"strict"},
                "MTX_RTSPSADDRESS":{"kind":"socket"},"MTX_RTSPSERVERCERT":{"kind":"path","value":certificate},
                "MTX_RTSPSERVERKEY":{"kind":"path","value":key},"MTX_PATHDEFAULTS_RECORDDELETEAFTER":{"kind":"duration"}},
            "defaults":{"MTX_WEBRTCADDITIONALHOSTS":source("url-host","/public_rtsp_publish_base_url","",""),
                "MTX_RTSPENCRYPTION":"strict","MTX_RTSPSADDRESS":":8322","MTX_RTSPSERVERCERT":certificate,
                "MTX_RTSPSERVERKEY":key,"MTX_PATHDEFAULTS_RECORDDELETEAFTER":"168h"},
            "duration":{"allow_plus":true,"units":[{"unit":"ns","scale_ns":1,"days":false},{"unit":"us","scale_ns":1000,"days":false},
                {"unit":"µs","scale_ns":1000,"days":false},{"unit":"μs","scale_ns":1000,"days":false},
                {"unit":"ms","scale_ns":1000000,"days":false},{"unit":"s","scale_ns":1000000000,"days":false},
                {"unit":"m","scale_ns":60000000000_u64,"days":false},{"unit":"h","scale_ns":3600000000000_u64,"days":false},
                {"unit":"d","scale_ns":86400000000000_u64,"days":true}],"minimum_nonzero_ns":"900000000000","maximum_ns":"9223372036854775807"}},
        "cases":environment_cases});
    let version = field("version=");
    let platform = field("platform=");
    let companion_cases = json!([
        {"name":"current-binary-and-contract","input":{"contract":lock,"stdout":format!("{version}\n"),"binary_sha256":hash},"valid":true},
        {"name":"wrong-current-version","input":{"contract":lock,"stdout":"v0.0.0\n","binary_sha256":hash},"valid":false},
        {"name":"wrong-platform","input":{"contract":lock.replace(platform,"unknown"),"stdout":version,"binary_sha256":hash},"valid":false},
        {"name":"wrong-binary","input":{"contract":lock,"stdout":version,"binary_sha256":"0".repeat(64)},"valid":false},
        {"name":"duplicate-contract-field","input":{"contract":format!("{lock}\nversion={version}\n"),"stdout":version,"binary_sha256":hash},"valid":false}
    ]);
    json!({"configuration_path":root("config/mediamtx.yml"),"template":asset(directory,"media-template.yml",template.as_bytes()),
        "substitutions":{"@@CALLBACK@@":{"source":callback,"encoding":"json-string"},"@@RECORD_PATH@@":{"source":records,"encoding":"json-string"}},
        "binary":{"path":"bin/mediamtx","sha256":hash},
        "contract":{"path":"share/mediamtx.lock","sha256":sha(lock.as_bytes())},
        "validation":[{"kind":"media-yaml-v1","description":"Current managed companion settings and anchored recording root",
            "parameters":{"scalars":scalars,"record_path":{"key":"recordPath","resource":"/resources/business_paths/0","separator":"/%path/"}},"cases":yaml_cases},
            environment,{"kind":"companion-contract-v1","description":"Pinned companion identity, digest and bounded version execution",
                "parameters":{"version":version,"platform":platform,"version_arguments":["--version"],"timeout_seconds":3,"maximum_output_bytes":256},"cases":companion_cases},
            {"kind":"pem-keypair-v1","description":"Existing operator-provided TLS certificate and matching private key",
                "parameters":{"certificate":root("config/rtsp-cert.pem"),"key":root("config/rtsp-key.pem"),"maximum_bytes":65536},
                "cases":serde_json::from_str::<Value>(include_str!("../schema/validation/pem-keypair.json")).unwrap()}]})
}

pub(crate) fn emit(
    destination: &Path,
    identity: &ReleaseIdentity,
    schema: &SchemaIdentity,
    ddl: &str,
) {
    let directory = destination.join("contracts");
    std::fs::create_dir_all(&directory).unwrap();
    let mut material =
        xcss::installation_material::common(identity, schema, Some("server-control-plane"))
            .unwrap();
    let definitions = material
        .as_object_mut()
        .unwrap()
        .remove("definitions")
        .unwrap();
    asset(
        &directory,
        "common-definitions.json",
        &serde_json::to_vec(&definitions).unwrap(),
    );
    let mut database = material
        .as_object_mut()
        .unwrap()
        .remove("database")
        .unwrap();
    database["name"] = json!("application");
    database["path"] = data("app.sqlite3");
    database["ddl"] = asset(&directory, "current-schema.sql", ddl.as_bytes());
    database["validation"] = json!([{"kind":"sgev-authorization-v1","description":"Current camera authorization envelope and plaintext digest",
        "parameters":{"query":"SELECT CAST(id AS BLOB) AS identity,authorization_code_enc AS ciphertext,authorization_code_hash AS digest FROM xcocs",
            "maximum_rows":2000000,"maximum_ciphertext_bytes":crate::persistent_crypto_definition::MAX_AUTHORIZATION_ENVELOPE_BYTES,
            "domain":crate::persistent_crypto_definition::AUTHORIZATION_DOMAIN,"revision":crate::persistent_crypto_definition::AUTHORIZATION_REVISION,
            "binding":"length-prefixed-uuid-text","key_secret":"instance.credentials_key","digest_encoding":"raw",
            "plaintext_bytes":crate::persistent_crypto_definition::AUTHORIZATION_CODE_BYTES,"nullable":false},
        "cases":serde_json::from_str::<Value>(include_str!("../schema/validation/authorization.json")).unwrap()}]);
    material["databases"] = json!([database]);
    let mut config =
        xcss::configuration_contract::generate::<crate::config_definition::Settings>().unwrap();
    config["schema"].as_object_mut().unwrap().remove("required");
    relocate(&mut config["defaults"]);
    config["data_dir"] = crate::config_definition::data_path_spec();
    config["bind_pointer"] = json!("/bind");
    config["rules"] = crate::config_definition::current_rules(true, true);
    for (field, path) in crate::config_definition::fixed_companion_paths() {
        config["rules"].as_array_mut().unwrap().push(
            json!({"kind":"path-equals","fields":[field],"arguments":{"expected":root(path)}}),
        );
    }
    let mut baseline = config["defaults"].clone();
    baseline["public_rtsp_publish_base_url"] = json!("rtsps://camera.example.org:8322");
    let mut cases = vec![
        json!({"name":"current-production-config","value":baseline,"valid":true}),
        json!({"name":"production-requires-an-explicit-public-origin","value":{},"valid":false}),
    ];
    for (name, field, value, valid) in [
        ("unknown-field", "retired_field", json!(1), false),
        ("private-data-required", "data_dir", Value::Null, false),
        (
            "relative-recordings",
            "recordings_directory",
            json!("relative"),
            false,
        ),
        (
            "declared-business-subtree",
            "recordings_directory",
            json!("$ROOT/state/recordings/archive"),
            true,
        ),
        (
            "business-exposes-secrets",
            "recordings_directory",
            json!("$ROOT/state/secrets.json.backup"),
            false,
        ),
        (
            "business-contains-private-state",
            "recordings_directory",
            json!("$ROOT"),
            false,
        ),
        (
            "protected-namespace",
            "data_dir",
            json!("$ROOT/maintenance/state"),
            false,
        ),
        ("invalid-log-level", "log_level", json!("invalid"), false),
        (
            "zero-callback-port",
            "media_callback_bind",
            json!("127.0.0.1:0"),
            false,
        ),
        (
            "public-callback",
            "media_callback_bind",
            json!("0.0.0.0:18107"),
            false,
        ),
        ("listener-overlap", "bind", json!("0.0.0.0:18107"), false),
        (
            "interval-under-bound",
            "request_timeout_secs",
            json!(0),
            false,
        ),
        (
            "interval-over-bound",
            "status_interval_secs",
            json!(3601),
            false,
        ),
        (
            "unsafe-production-rtsp",
            "public_rtsp_publish_base_url",
            json!("rtsp://camera.example.org"),
            false,
        ),
        (
            "loopback-production-origin",
            "public_rtsp_publish_base_url",
            json!("rtsps://127.0.0.1"),
            false,
        ),
        (
            "ipv6-loopback-production-origin",
            "public_rtsp_publish_base_url",
            json!("rtsps://[::1]"),
            false,
        ),
        (
            "fixed-companion-layout",
            "mediamtx_binary",
            json!("$ROOT/bin/unknown"),
            false,
        ),
        (
            "bound-release-external-web",
            "static_dir",
            json!("$ROOT/external-web"),
            false,
        ),
    ] {
        let mut candidate = baseline.clone();
        candidate[field] = value;
        cases.push(json!({"name":name,"value":candidate,"valid":valid}));
    }
    config["cases"] = json!(cases);
    material["config"] = config;
    use base64::Engine as _;
    let public_key = base64::engine::general_purpose::STANDARD
        .encode([9_u8; crate::secrets_definition::KEY_BYTES]);
    let sample = json!({"format":crate::secrets_definition::FORMAT,"jwt_secret":public_key,"credentials_key":public_key});
    let secret_schema = schemars::schema_for!(crate::secrets_definition::InstanceSecrets);
    let fields=secret_schema.as_object().unwrap()["properties"].as_object().unwrap().iter()
        .filter(|(_,value)|value["type"]=="string").map(|(name,_)|json!({"name":name,"bytes":crate::secrets_definition::KEY_BYTES,"encoding":"base64"})).collect::<Vec<_>>();
    material["secrets"] = json!([{"name":"instance","path":data("secrets.json"),"format":"json","fields":fields,
        "constants":{"format":crate::secrets_definition::FORMAT},"validation":[],"cases":[
            {"name":"current-instance-secrets","input":sample,"valid":true},
            {"name":"missing-keys","input":{"format":1},"valid":false},
            {"name":"wrong-format","input":{"format":0,"jwt_secret":public_key,"credentials_key":public_key},"valid":false},
            {"name":"wrong-key-length","input":{"format":1,"jwt_secret":"AA==","credentials_key":public_key},"valid":false},
            {"name":"unknown-secret-field","input":{"format":1,"jwt_secret":public_key,"credentials_key":public_key,"removed":0},"valid":false}]}]);
    let state = crate::config_definition::data_path_spec();
    let records = crate::config_definition::recordings_path_spec();
    material["resources"] = json!({"state_paths":[state,root("config"),records],"writable_paths":[state,records,root("logs"),root("run")],
        "readonly_paths":[root("config/config.json"),root("config/mediamtx.yml"),root("config/mediamtx.env"),root("config/rtsp-cert.pem"),root("config/rtsp-key.pem")],
        "shared_paths":[],"business_paths":[records],"business_directories":[],"config_write_paths":[],"private_directories":[state,root("logs"),root("run")],
        "runtime_locks":[{"path":data(".state-instance.lock"),"kind":"file"}]});
    material["writers"] = json!([{"role":"server","service":"xcos.service","configuration_paths":[root("config/config.json")]},
        {"role":"media","service":"xcos-mediamtx.service","configuration_paths":[root("config/mediamtx.yml"),root("config/mediamtx.env")]}]);
    material["media"] = media(&directory);
    for (path, bytes) in [
        (
            "src/config_definition.rs",
            include_bytes!("config_definition.rs").as_slice(),
        ),
        (
            "src/persistent_crypto_definition.rs",
            include_bytes!("persistent_crypto_definition.rs").as_slice(),
        ),
        (
            "src/secrets_definition.rs",
            include_bytes!("secrets_definition.rs").as_slice(),
        ),
        (
            "src/installation_definition.rs",
            include_bytes!("installation_definition.rs").as_slice(),
        ),
        (
            "schema/product.sql",
            include_bytes!("../schema/product.sql").as_slice(),
        ),
        (
            "schema/validation/authorization.json",
            include_bytes!("../schema/validation/authorization.json").as_slice(),
        ),
        (
            "schema/validation/pem-keypair.json",
            include_bytes!("../schema/validation/pem-keypair.json").as_slice(),
        ),
        (
            "config/mediamtx.lock",
            include_bytes!("../config/mediamtx.lock").as_slice(),
        ),
        (
            "config/mediamtx.yml",
            include_bytes!("../config/mediamtx.yml").as_slice(),
        ),
    ] {
        material["authority_sources"].as_array_mut().unwrap().push(json!({"path":path,"sha256":sha(bytes),"generator":format!("xcos/build.rs@{}",identity.source_revision)}));
    }
    asset(
        &directory,
        "install.json",
        &serde_json::to_vec(&material).unwrap(),
    );
}
