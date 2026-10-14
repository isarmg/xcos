//! Product-owned database identity shared by runtime and build metadata.
pub const APPLICATION: &str = "xcos";
pub const APPLICATION_VERSION: &str = "xcos-db-v1";
pub const SCHEMA_REVISION: i64 = 1;

// Build-time installation material and test initialization share this data
// definition; the running service never inserts an initial lease row.
#[allow(dead_code)]
pub const INITIAL_RECONCILER_LEASE_SQL: &str = "INSERT INTO media_reconciler_leases(singleton,updated_at) VALUES(1,'1970-01-01T00:00:00+00:00')";

pub fn identity(
    sha256: &str,
) -> Result<xcss::schema_identity::SchemaIdentity, xcss::schema_identity::Error> {
    xcss::schema_identity::SchemaIdentity::new(
        APPLICATION,
        APPLICATION_VERSION,
        SCHEMA_REVISION as u64,
        sha256,
    )
}
