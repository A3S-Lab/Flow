#[cfg(feature = "postgres")]
mod postgres;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
mod scope_cancel;
#[cfg(any(feature = "postgres", feature = "sqlite"))]
mod scope_cancel_open;
#[cfg(feature = "postgres")]
mod select_timer;
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
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SELECT_TIMER_SQL: &str =
    select_timer::POSTGRES_SCHEDULED_WAKEUPS_SELECT_TIMER_SQL;
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SCOPE_CANCEL_SQL: &str =
    scope_cancel::POSTGRES_SCHEDULED_WAKEUPS_SCOPE_CANCEL_SQL;
#[cfg(feature = "postgres")]
pub(super) const POSTGRES_SCHEDULED_WAKEUPS_SCOPE_CANCEL_OPEN_SQL: &str =
    scope_cancel_open::POSTGRES_SCHEDULED_WAKEUPS_SCOPE_CANCEL_OPEN_SQL;
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
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SELECT_TIMER_SQL: &str =
    sqlite::SQLITE_SCHEDULED_WAKEUPS_SELECT_TIMER_SQL;
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SCOPE_CANCEL_SQL: &str =
    scope_cancel::SQLITE_SCHEDULED_WAKEUPS_SCOPE_CANCEL_SQL;
#[cfg(feature = "sqlite")]
pub(super) const SQLITE_SCHEDULED_WAKEUPS_SCOPE_CANCEL_OPEN_SQL: &str =
    scope_cancel_open::SQLITE_SCHEDULED_WAKEUPS_SCOPE_CANCEL_OPEN_SQL;
