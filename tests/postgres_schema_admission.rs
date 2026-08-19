#![cfg(feature = "postgres")]

use a3s_flow::{
    migrate_postgres_flow, FlowEventStore, FlowTaskQueue, PostgresEventStore, PostgresFlowTaskQueue,
};
use a3s_orm::PostgresExecutor;
use uuid::Uuid;

fn postgres_url_from_env() -> Option<String> {
    std::env::var("A3S_FLOW_POSTGRES_URL")
        .ok()
        .filter(|url| !url.trim().is_empty())
}

fn schema_scoped_url(postgres_url: &str, schema: &str) -> String {
    let separator = if postgres_url.contains('?') { '&' } else { '?' };
    format!("{postgres_url}{separator}options=-csearch_path%3D{schema}")
}

#[tokio::test]
async fn dedicated_migrator_admits_store_and_queue_without_serving_ddl() {
    let Some(postgres_url) = postgres_url_from_env() else {
        eprintln!("skipping PostgreSQL schema admission test; set A3S_FLOW_POSTGRES_URL");
        return;
    };
    let schema = format!("flow_schema_admission_{}", Uuid::new_v4().simple());
    let base_executor = PostgresExecutor::connect_no_tls(&postgres_url, 2).unwrap();
    base_executor
        .connection()
        .await
        .unwrap()
        .batch_execute(&format!("CREATE SCHEMA {schema}"))
        .await
        .unwrap();

    let scoped_url = schema_scoped_url(&postgres_url, &schema);
    let scoped_executor = PostgresExecutor::connect_no_tls(&scoped_url, 2).unwrap();
    let admission_error = PostgresEventStore::from_executor_verified(scoped_executor.clone())
        .await
        .expect_err("an unmigrated serving schema must fail admission");
    assert!(admission_error
        .to_string()
        .contains("PostgreSQL Flow schema admission failed"));

    let ledger: Option<String> = scoped_executor
        .connection()
        .await
        .unwrap()
        .query_one("SELECT to_regclass('a3s_orm_migrations')::text", &[])
        .await
        .unwrap()
        .get(0);
    assert_eq!(ledger, None, "serving admission must not create the ledger");

    let report = migrate_postgres_flow(&scoped_executor).await.unwrap();
    assert!(!report.applied.is_empty());
    let store = PostgresEventStore::from_executor_verified(scoped_executor.clone())
        .await
        .unwrap();
    let queue = PostgresFlowTaskQueue::from_executor_verified_with_queue(
        scoped_executor.clone(),
        "admitted",
    )
    .await
    .unwrap();
    assert!(store.list_run_ids().await.unwrap().is_empty());
    assert_eq!(queue.len().await.unwrap(), 0);

    drop(queue);
    drop(store);
    drop(scoped_executor);
    base_executor
        .connection()
        .await
        .unwrap()
        .batch_execute(&format!("DROP SCHEMA {schema} CASCADE"))
        .await
        .unwrap();
}
