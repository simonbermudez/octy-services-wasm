//! octy-data-gateway — native, **multi-tenant** sidecar that bridges every
//! WASM service component to backends that need raw TCP or native SDKs:
//!
//!   * MongoDB   → generic collection CRUD over HTTP (legacy extended JSON)
//!   * RabbitMQ  → `/v1/amqp/publish` + a consumer loop that forwards
//!                 deliveries to the component as HTTP POSTs
//!   * AWS S3    → the BucketRepository operations
//!
//! One gateway process now serves every service (previously: one gateway
//! deployment per service). Each request identifies its caller with the
//! `X-Octy-Service` header (set automatically by every service's
//! `octy_spin::gateway::GatewayClient`); the gateway looks up that service's
//! own MongoDB connection and AMQP consumer bindings from `GATEWAY_TENANTS`.
//! AWS credentials and the AMQP connection/exchange are gateway-wide, since
//! S3 bucket names and Mongo collection names already travel per-request
//! (nothing bucket- or collection-shaped was ever tenant config), and every
//! service already shared one RabbitMQ instance and topic exchange.
//!
//! Configuration (environment):
//!   PORT                  HTTP port (default 8090)
//!   GATEWAY_TENANTS       JSON array, one entry per service — see `TenantSpec`
//!   AMQP_URL              RabbitMQ URL (optional — AMQP disabled if unset)
//!   AMQP_EXCHANGE         topic exchange name (default "octy-services")
//!   AWS_REGION / AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY   for S3
//!   AWS_ENDPOINT_URL / S3_FORCE_PATH_STYLE   for a local MinIO substitute

mod amqp;
mod ejson;
mod mongo;
mod s3;

use std::collections::HashMap;
use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;

/// One entry of `GATEWAY_TENANTS` — the same `DB_URI`/`AMQP_CONSUMERS`/
/// `AMQP_FORWARD_URL` fields every per-service gateway deployment used to
/// carry individually, now grouped by `service` inside one shared blob.
#[derive(Deserialize)]
pub struct TenantSpec {
    pub service: String,
    /// Omit for a service with no MongoDB usage at all (e.g. `configurations`,
    /// which only ever publishes to AMQP).
    #[serde(default)]
    pub db_uri: Option<String>,
    /// AMQP routing keys this service consumes. Each becomes a queue named
    /// `{service}.{routing_key}`, matching the old `AMQP_QUEUE_PREFIX`
    /// convention, bound to the gateway-wide exchange.
    #[serde(default)]
    pub routing_keys: Vec<String>,
    /// Required if `routing_keys` is non-empty; where deliveries are forwarded.
    #[serde(default)]
    pub forward_url: Option<String>,
}

pub struct TenantState {
    pub db: Option<mongodb::Database>,
}

pub struct AppState {
    pub tenants: HashMap<String, TenantState>,
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

    let tenant_specs: Vec<TenantSpec> = serde_json::from_str(&std::env::var("GATEWAY_TENANTS")?)
        .map_err(|e| anyhow::anyhow!("GATEWAY_TENANTS must be a JSON array of tenant objects: {e}"))?;

    // Reuse one MongoDB Client per distinct db_uri (several tenants commonly
    // point at the same cluster), rather than opening one connection pool
    // per tenant regardless of overlap.
    let mut clients_by_uri: HashMap<String, mongodb::Client> = HashMap::new();
    let mut tenants: HashMap<String, TenantState> = HashMap::new();

    for spec in &tenant_specs {
        let db = match &spec.db_uri {
            None => None,
            Some(uri) => {
                if !clients_by_uri.contains_key(uri) {
                    let client = mongodb::Client::with_uri_str(uri).await?;
                    clients_by_uri.insert(uri.clone(), client);
                }
                let client = clients_by_uri.get(uri).expect("just inserted");
                let db = client
                    .default_database()
                    .ok_or_else(|| anyhow::anyhow!("{}'s db_uri must include a default database", spec.service))?;
                tracing::info!("{}: connected to MongoDB (db: {})", spec.service, db.name());
                Some(db)
            }
        };
        tenants.insert(spec.service.clone(), TenantState { db });
    }

    let amqp = match std::env::var("AMQP_URL") {
        Ok(url) => {
            let exchange = std::env::var("AMQP_EXCHANGE").unwrap_or_else(|_| "octy-services".to_string());
            let publisher = amqp::Publisher::connect(&url, &exchange).await?;
            amqp::spawn_consumers(&url, &exchange, &tenant_specs).await?;
            Some(publisher)
        }
        Err(_) => {
            tracing::warn!("AMQP_URL not set — AMQP publish/consume disabled for all tenants");
            None
        }
    };

    let s3 = s3::S3Buckets::from_env().await;

    let state: SharedState = Arc::new(AppState { tenants, amqp, s3 });

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
    tracing::info!("octy-data-gateway listening on :{port} ({} tenants)", tenant_specs.len());
    axum::serve(listener, app).await?;
    Ok(())
}

async fn healthz(State(_): State<SharedState>) -> Json<serde_json::Value> {
    Json(json!("OK"))
}
