#[path = "src/config_definition.rs"]
#[allow(dead_code)] // Runtime-only fields are intentionally absent from exported configuration.
mod config_definition;
#[path = "src/installation_definition.rs"]
mod installation_definition;
#[path = "src/persistent_crypto_definition.rs"]
mod persistent_crypto_definition;
#[path = "src/schema_definition.rs"]
mod schema_definition;
#[path = "src/secrets_definition.rs"]
mod secrets_definition;
use std::{collections::BTreeSet, env, fs, path::PathBuf};

const XCSS_SOURCE: &str = "git+https://github.com/isarmg/xcss.git?rev=";

fn locked_xcss_revision(lockfile: &str) -> String {
    let revisions = lockfile
        .lines()
        .map(str::trim)
        .filter_map(|line| line.strip_prefix("source = \"")?.strip_suffix('"'))
        .filter_map(|source| source.strip_prefix(XCSS_SOURCE))
        .map(|source| source.split_once('#').expect("locked xcss source"))
        .map(|(requested, locked)| {
            assert_eq!(requested, locked, "xcss revision must be immutable");
            assert!(
                locked.len() == 40 && locked.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "xcss revision must be full hexadecimal"
            );
            locked.to_owned()
        })
        .collect::<BTreeSet<_>>();
    if revisions.is_empty()
        && std::env::var("XCOS_SOURCE_REVISION").unwrap_or_else(|_| "unbound".into()) == "unbound"
        && std::env::var("PROFILE").is_ok_and(|profile| profile == "debug")
        && std::env::var_os("XSSI_BUILD_METADATA_DIR").is_none()
    {
        return "unbound".into();
    }
    assert_eq!(
        revisions.len(),
        1,
        "the xcss package must use one immutable revision"
    );
    revisions.into_iter().next().expect("one xcss revision")
}

fn main() {
    println!("cargo:rerun-if-env-changed=XSSI_BUILD_METADATA_DIR");
    println!("cargo:rerun-if-env-changed=XCSS_WEB_DIST");
    println!("cargo:rerun-if-env-changed=XCOS_SOURCE_REVISION");
    println!("cargo:rerun-if-changed=src/schema_definition.rs");
    let schema = [
        xcss::schema_identity::PRODUCT_METADATA_DDL,
        xcss::platform_db::PLATFORM_METADATA_DDL,
        xcss::admin_sqlite::ADMIN_PERSISTENT_DDL,
        xcss::operations::PLATFORM_OPERATIONS_DDL,
        include_str!("schema/product.sql"),
    ]
    .into_iter()
    .map(|definition| format!("{};\n", definition.trim().trim_end_matches(';')))
    .collect::<String>();
    let schema_hash =
        xcss::sqlite::fingerprint_trusted_ddl(&[&schema]).expect("current schema fingerprint");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("current-schema.sql"),
        schema,
    )
    .unwrap();
    println!("cargo:rustc-env=XCOS_CURRENT_SCHEMA_SHA256={schema_hash}");
    println!("cargo:rerun-if-changed=schema/product.sql");

    let target = env::var("TARGET").expect("Cargo must provide TARGET");
    assert_eq!(
        target, "x86_64-unknown-linux-gnu",
        "Xcos server builds support only x86_64-unknown-linux-gnu"
    );
    let source_revision = env::var("XCOS_SOURCE_REVISION").unwrap_or_else(|_| "unbound".to_owned());
    assert!(
        (source_revision == "unbound"
            && env::var("PROFILE").is_ok_and(|profile| profile == "debug")
            && env::var_os("XSSI_BUILD_METADATA_DIR").is_none())
            || (source_revision.len() == 40
                && source_revision
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))),
        "XCOS_SOURCE_REVISION must be unbound or a full lowercase 40-hex Git commit"
    );
    println!("cargo:rustc-env=XCOS_BUILD_TARGET={target}");
    println!("cargo:rustc-env=XCOS_SOURCE_REVISION={source_revision}");
    let xcss_revision =
        locked_xcss_revision(&fs::read_to_string("Cargo.lock").expect("read Cargo.lock"));
    println!("cargo:rustc-env=XCSS_REVISION={xcss_revision}");
    println!("cargo:rerun-if-changed=Cargo.lock");

    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("Cargo manifest directory"));
    let protocol_path = root.join("web/src/protocol-contract.json");
    println!("cargo:rerun-if-changed={}", protocol_path.display());
    let protocol: serde_json::Value =
        serde_json::from_slice(&fs::read(protocol_path).expect("read product protocol authority"))
            .expect("product protocol must be valid JSON");
    let edge = protocol["edge_protocol"]
        .as_str()
        .expect("edge protocol identity is required");
    assert_eq!(
        edge, "xcos-edge-v1",
        "only the current product edge contract is implemented"
    );
    println!("cargo:rustc-env=XCOS_EDGE_PROTOCOL={edge}");
    let web_root = env::var_os("XCSS_WEB_DIST")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("web/dist"));
    xcss::web_assets::build::generate(&web_root).expect("build current embedded Web assets");
    emit_offline_metadata();
}

fn emit_offline_metadata() {
    use sha2::{Digest, Sha256};
    use xcss::contracts::{StateContract, StateResource, StateResourceKind, StateSchemaIdentity};
    let Some(destination) = std::env::var_os("XSSI_BUILD_METADATA_DIR") else {
        return;
    };
    let destination = std::path::PathBuf::from(destination);
    assert!(
        destination.is_absolute() && destination.is_dir(),
        "explicit metadata destination must exist and be absolute"
    );
    let revision = std::env::var("XCOS_SOURCE_REVISION")
        .expect("release metadata requires a source-bound build");
    let ddl = fs::read_to_string(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("current-schema.sql"),
    )
    .unwrap();
    let hash = xcss::sqlite::fingerprint_trusted_ddl(&[&ddl]).unwrap();
    let schema = schema_definition::identity(&hash).expect("schema identity");
    let contract = StateContract {
        contract_version: xcss::contracts::STATE_CONTRACT_VERSION,
        application: "xcos".into(),
        application_version: std::env::var("CARGO_PKG_VERSION").unwrap(),
        source_revision: revision.clone(),
        schema: Some(StateSchemaIdentity {
            revision: schema.schema_revision,
            sha256: hash.clone(),
        }),
        maintenance_locks: vec![".state-instance.lock".into()],
        resources: vec![
            StateResource {
                name: "state".into(),
                kind: StateResourceKind::DataTree,
                required: true,
            },
            StateResource {
                name: "recordings".into(),
                kind: StateResourceKind::Recordings,
                required: true,
            },
            StateResource {
                name: "config".into(),
                kind: StateResourceKind::Configuration,
                required: true,
            },
        ],
        external_requirements: vec![],
        companion_contracts: {
            let lock = include_str!("config/mediamtx.lock");
            let get = |key: &str| {
                lock.lines()
                    .find_map(|line| line.strip_prefix(key))
                    .expect("media lock")
                    .to_owned()
            };
            vec![xcss::contracts::CompanionContract {
                name: "mediamtx".into(),
                version: get("version="),
                platform: get("platform="),
                sha256: get("sha256="),
            }]
        },
    };
    contract.validate().expect("current state contract");
    let bytes = serde_json::to_vec(&contract).unwrap();
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let identity = xcss::contracts::ReleaseIdentity {
        product: "xcos".into(),
        version: std::env::var("CARGO_PKG_VERSION").unwrap(),
        source_revision: revision,
        target: xcss::server_target::SERVER_TARGET_TRIPLE.into(),
        state_contract_sha256: digest,
    };
    identity.validate().expect("release identity");
    installation_definition::emit(&destination, &identity, &schema, &ddl);
    std::fs::write(
        destination.join("xcos-identity.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .unwrap();
    std::fs::write(
        destination.join("xcos-schema.json"),
        serde_json::to_vec(&schema).unwrap(),
    )
    .unwrap();
    std::fs::write(destination.join("xcos-state-contract.json"), bytes).unwrap();
    let generated = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap())
        .join("xcss-web-assets.rs.manifest.json");
    std::fs::copy(generated, destination.join("xcos-web-assets.json"))
        .expect("copy exact embedded Web manifest");
}
