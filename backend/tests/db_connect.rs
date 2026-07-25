//! Startup-schema concurrency regression test.
//!
//! `CREATE TABLE IF NOT EXISTS` can still race in PostgreSQL when separate
//! sessions initialize a completely empty database simultaneously. Production
//! replicas and Rust's parallel integration tests can both create that
//! situation, so `Db::connect` must serialize schema application.

use gitdebt::db::Db;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};

async fn isolated_database(suffix: &str) -> Option<(PgPool, String, String)> {
    let Ok(admin_url) = std::env::var("GITDEBT_TEST_DATABASE_URL") else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return None;
    };

    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url)
        .await
        .expect("connect to test Postgres");
    let database_name = format!("gitdebt_connect_test_{}_{}", std::process::id(), suffix);
    let create = format!(r#"CREATE DATABASE "{database_name}""#);
    let drop = format!(r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#);

    sqlx::query(sqlx::AssertSqlSafe(drop.as_str()))
        .execute(&admin)
        .await
        .expect("drop stale test database");
    sqlx::query(sqlx::AssertSqlSafe(create.as_str()))
        .execute(&admin)
        .await
        .expect("create test database");

    let mut database_url = url::Url::parse(&admin_url).expect("parse test database URL");
    database_url.set_path(&format!("/{database_name}"));
    Some((admin, database_name, database_url.to_string()))
}

async fn drop_database(admin: PgPool, database_name: &str) {
    let drop = format!(r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#);
    sqlx::query(sqlx::AssertSqlSafe(drop.as_str()))
        .execute(&admin)
        .await
        .expect("drop test database");
    admin.close().await;
}

async fn hold_write_lock(db: &Db) -> Transaction<'_, Postgres> {
    let mut transaction = db.pool.begin().await.expect("begin blocker transaction");
    sqlx::query("LOCK TABLE repo_author_commit_days IN ROW EXCLUSIVE MODE")
        .execute(&mut *transaction)
        .await
        .expect("lock analyzed data table");
    transaction
}

#[tokio::test]
async fn concurrent_first_connects_serialize_schema_creation() {
    let Some((admin, database_name, database_url)) = isolated_database("concurrent").await else {
        return;
    };

    let results = futures::future::join_all((0..6).map(|_| Db::connect(&database_url))).await;

    for db in results.iter().filter_map(|result| result.as_ref().ok()) {
        db.pool.close().await;
    }
    drop_database(admin, &database_name).await;

    for result in results {
        result.expect("every concurrent Db::connect should apply the schema");
    }
}

#[tokio::test]
async fn current_schema_connect_does_not_wait_for_writer_locks() {
    let Some((admin, database_name, database_url)) = isolated_database("current").await else {
        return;
    };
    let initialized = Db::connect(&database_url)
        .await
        .expect("initialize test schema");
    let version: i32 = sqlx::query_scalar("SELECT version FROM schema_version WHERE id = 1")
        .fetch_one(&initialized.pool)
        .await
        .expect("read installed schema version");
    assert_eq!(version, 4);

    let blocker = hold_write_lock(&initialized).await;
    let connected = tokio::time::timeout(
        std::time::Duration::from_secs(8),
        Db::connect(&database_url),
    )
    .await
    .expect("current schema startup must not wait for DDL locks")
    .expect("connect with current schema");

    connected.pool.close().await;
    blocker.rollback().await.expect("release write lock");
    initialized.pool.close().await;
    drop_database(admin, &database_name).await;
}
