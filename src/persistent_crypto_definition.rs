//! Product-owned persistent authorization envelope definition.
pub(crate) const AUTHORIZATION_DOMAIN: &str = "xcos/client-authorization";
pub(crate) const AUTHORIZATION_REVISION: u16 = 1;
pub(crate) const MAX_AUTHORIZATION_ENVELOPE_BYTES: usize = 1024;
pub(crate) const AUTHORIZATION_CODE_BYTES: usize = 36;
