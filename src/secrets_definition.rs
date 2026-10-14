//! Current private instance-secret document, shared by parsing and material export.
use serde::{Deserialize, Serialize};
pub(crate) const FORMAT: u32 = 1;
pub(crate) const KEY_BYTES: usize = 32;
#[derive(Serialize, Deserialize, schemars::JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstanceSecrets {
    pub(crate) format: u32,
    pub(crate) jwt_secret: String,
    pub(crate) credentials_key: String,
}
