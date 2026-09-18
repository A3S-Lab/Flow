#[cfg(feature = "postgres")]
mod postgres;
#[cfg(feature = "sqlite")]
mod sqlite;

#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SQL: &str = postgres::POSTGRES_SCHEDULED_WAKEUPS_SQL;
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_CANCELLATION_SQL: &str =
    postgres::POSTGRES_SCHEDULED_WAKEUPS_CANCELLATION_SQL;
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_ACTIVITY_RETRY_SQL: &str =
    postgres::POSTGRES_SCHEDULED_WAKEUPS_ACTIVITY_RETRY_SQL;
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_ACTIVITY_KIND_SQL: &str =
    postgres::POSTGRES_SCHEDULED_WAKEUPS_ACTIVITY_KIND_SQL;
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SQL: &str = sqlite::SQLITE_SCHEDULED_WAKEUPS_SQL;
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_CANCELLATION_SQL: &str =
    sqlite::SQLITE_SCHEDULED_WAKEUPS_CANCELLATION_SQL;
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_ACTIVITY_RETRY_SQL: &str =
    sqlite::SQLITE_SCHEDULED_WAKEUPS_ACTIVITY_RETRY_SQL;
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_ACTIVITY_KIND_SQL: &str =
    sqlite::SQLITE_SCHEDULED_WAKEUPS_ACTIVITY_KIND_SQL;
