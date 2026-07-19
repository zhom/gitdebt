//! Startup-schema concurrency regression test.
//!
//! `CREATE TABLE IF NOT EXISTS` can still race in PostgreSQL when separate
//! sessions initialize a completely empty database simultaneously. Production
//! replicas and Rust's parallel integration tests can both create that
//! situation, so `Db::connect` must serialize schema application.

use gitdebt::db::Db;
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn concurrent_first_connects_serialize_schema_creation() {
    let Ok(admin_url) = std::env::var("GITDEBT_TEST_DATABASE_URL") else {
        eprintln!("skipping: set GITDEBT_TEST_DATABASE_URL to run");
        return;
    };

    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect(&admin_url)
        .await
        .expect("connect to test Postgres");
    let database_name = format!("gitdebt_connect_test_{}", std::process::id());
    let create = format!(r#"CREATE DATABASE "{database_name}""#);
    let drop = format!(r#"DROP DATABASE IF EXISTS "{database_name}" WITH (FORCE)"#);

    // Clean up a database left by a killed prior test process, then recreate a
    // genuinely empty catalog so all connect tasks hit the DDL path.
    // The identifier is generated exclusively from this process's numeric PID;
    // no external input reaches either audited dynamic statement.
    sqlx::query(sqlx::AssertSqlSafe(drop.as_str()))
        .execute(&admin)
        .await
        .expect("drop stale concurrency test database");
    sqlx::query(sqlx::AssertSqlSafe(create.as_str()))
        .execute(&admin)
        .await
        .expect("create concurrency test database");

    let mut database_url = url::Url::parse(&admin_url).expect("parse test database URL");
    database_url.set_path(&format!("/{database_name}"));
    let database_url = database_url.to_string();

    let results = futures::future::join_all((0..6).map(|_| Db::connect(&database_url))).await;

    for db in results.iter().filter_map(|result| result.as_ref().ok()) {
        db.pool.close().await;
    }
    sqlx::query(sqlx::AssertSqlSafe(drop.as_str()))
        .execute(&admin)
        .await
        .expect("drop concurrency test database");
    admin.close().await;

    for result in results {
        result.expect("every concurrent Db::connect should apply the schema");
    }
}
