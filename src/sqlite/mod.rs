use xcss::schema_identity::SchemaIdentity;

mod connection;
mod lease;
mod paths;

pub(crate) use connection::{
    bounded_columns, current_validation_snapshot, history_connection, history_error,
    open_runtime_pool,
};
#[cfg(test)]
pub(crate) use connection::{
    initialize_current_database, initialize_test_pool, open_pool, validate_current_database,
};
pub(crate) use lease::{
    validate_global_lease_schema_sql, validate_global_lease_values, GlobalLeaseState,
};
pub(crate) use paths::database_path;

#[cfg(test)]
const APPLICATION: &str = crate::schema_definition::APPLICATION;
#[cfg(test)]
pub const CURRENT_SCHEMA_REVISION: i64 = crate::schema_definition::SCHEMA_REVISION;
pub const CURRENT_SCHEMA_SHA256: &str = env!("XCOS_CURRENT_SCHEMA_SHA256");
#[cfg(test)]
pub(crate) const CURRENT_SCHEMA: &str =
    include_str!(concat!(env!("OUT_DIR"), "/current-schema.sql"));

pub(crate) fn current_schema_identity() -> anyhow::Result<SchemaIdentity> {
    crate::schema_definition::identity(CURRENT_SCHEMA_SHA256).map_err(Into::into)
}

#[cfg(test)]
mod tests;
