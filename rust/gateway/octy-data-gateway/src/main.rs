//! octy-data-gateway — native sidecar that bridges the WASM service
//! components to backends that need raw TCP or native SDKs:
//!
//!   * MongoDB   → generic collection CRUD over HTTP (legacy extended JSON)
//!   * RabbitMQ  → `/v1/amqp/publish` + a consumer loop that forwards
//!                 deliveries to the component as HTTP POSTs
//!   * AWS S3    → the BucketRepository operations
//!
//! Configuration (environment):
//!   PORT                  HTTP port (default 8090)
//!   DB_URI                MongoDB connection string (default database in path)
//!   AMQP_URL              RabbitMQ URL (optional — AMQP disabled if unset)
//!   AMQP_EXCHANGE         topic exchange name
//!   AMQP_CONSUMERS        JSON array of routing keys to consume
//!   AMQP_QUEUE_PREFIX     queue name prefix (default "account")
//!   AMQP_FORWARD_URL      component URL deliveries are POSTed to
//!   AWS_REGION / AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY   for S3

mod amqp;
mod ejson;
mod mongo;
mod s3;

use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde_json::json;

pub struct AppState {
    pub db: mongodb::Database,
    pub amqp: Option<amqp::Publisher>,
    pub s3: s3::S3Buckets,
}

pub type SharedState = Arc<AppState>;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info".into()),
        )
        .init();

    let db_uri = std::env::var("DB_URI")?;
    let client = mongodb::Client::with_uri_str(&db_uri).await?;
    let db = client
        .default_database()
        .ok_or_else(|| anyhow::anyhow!("DB_URI must include a default database"))?;
    tracing::info!("connected to MongoDB (db: {})", db.name());

    let amqp = match std::env::var("AMQP_URL") {
        Ok(url) => {
            let exchange = std::env::var("AMQP_EXCHANGE").unwrap_or_else(|_| "octy".to_string());
            let publisher = amqp::Publisher::connect(&url, &exchange).await?;
            amqp::spawn_consumers(&url, &exchange).await?;
            Some(publisher)
        }
        Err(_) => {
            tracing::warn!("AMQP_URL not set — AMQP publish/consume disabled");
            None
        }
    };

    let s3 = s3::S3Buckets::from_env().await;

    let state: SharedState = Arc::new(AppState { db, amqp, s3 });

    let app = Router::new()
        .route("/healthz", get(healthz))
        .route("/v1/mongo/:collection/find-one", post(mongo::find_one))
        .route("/v1/mongo/:collection/find", post(mongo::find))
        .route("/v1/mongo/:collection/count", post(mongo::count))
        .route("/v1/mongo/:collection/insert-one", post(mongo::insert_one))
        .route("/v1/mongo/:collection/insert-many", post(mongo::insert_many))
        .route("/v1/mongo/:collection/update-one", post(mongo::update_one))
        .route("/v1/mongo/:collection/update-many", post(mongo::update_many))
        .route("/v1/mongo/:collection/delete-one", post(mongo::delete_one))
        .route("/v1/mongo/:collection/delete-many", post(mongo::delete_many))
        .route("/v1/mongo/:collection/aggregate", post(mongo::aggregate))
        .route("/v1/amqp/publish", post(amqp::publish))
        .route("/v1/s3/create-bucket", post(s3::create_bucket))
        .route("/v1/s3/configure-bucket", post(s3::configure_bucket))
        .route("/v1/s3/create-directory", post(s3::create_directory))
        .route("/v1/s3/delete-bucket", post(s3::delete_bucket))
        .route("/v1/s3/put-object", post(s3::put_object))
        .route("/v1/s3/get-object", post(s3::get_object))
        .route("/v1/s3/list-objects", post(s3::list_objects))
        .route("/v1/s3/delete-object", post(s3::delete_object))
        .with_state(state);

    let port: u16 = std::env::var("PORT").ok().and_then(|p| p.parse().ok()).unwrap_or(8090);
    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port)).await?;
    tracing::info!("octy-data-gateway listening on :{port}");
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz(State(_): State<SharedState>) -> Json<serde_json::Value> {
    Json(json!("OK"))
}
