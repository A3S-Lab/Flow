use a3s_orm::{MigrationReport, Migrator, PostgresExecutor};

use crate::error::{FlowError, Result};

use super::postgres_migrations;

/// Apply the canonical Flow PostgreSQL schema with a dedicated migration
/// executor.
///
/// Production hosts should terminate this operation before starting serving
/// workers, then construct stores and queues through their `*_verified`
/// constructors with a role that does not have DDL authority.
pub async fn migrate_postgres_flow(executor: &PostgresExecutor) -> Result<MigrationReport> {
    Migrator::new(executor.clone())
        .run(postgres_migrations())
        .await
        .map_err(|error| FlowError::Store(format!("PostgreSQL Flow migration failed: {error}")))
}

pub(crate) async fn verify_postgres_flow(executor: &PostgresExecutor) -> Result<()> {
    Migrator::new(executor.clone())
        .verify_required(postgres_migrations())
        .await
        .map_err(|error| {
            FlowError::Store(format!(
                "PostgreSQL Flow schema admission failed: {error}; run migrate_postgres_flow before serving"
            ))
        })
}
