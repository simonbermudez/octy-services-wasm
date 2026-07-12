//! RabbitMQ bridge — gateway-wide connection and exchange, shared by every
//! tenant service.
//!
//! Publish: `POST /v1/amqp/publish {"routing_key": ..., "payload": ...}` —
//! replaces `octy_rabbitmq.amqp_publisher` for the WASM components. Any
//! tenant can publish any routing key; the exchange doesn't care who sent it.
//!
//! Consume: each tenant's `routing_keys` (from `GATEWAY_TENANTS`) gets its own
//! queue (`{service}.{routing_key}`) bound to the topic exchange; deliveries
//! are forwarded to that tenant's own `forward_url` as
//! `POST {forward_url} {"routing_key": ..., "payload": ...}`.
//! Component response drives the acknowledgement (replaces `amqp/consumer.py`):
//!   2xx → ack, 4xx → reject (no requeue), anything else → reject + requeue.

use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use futures_util::StreamExt;
use lapin::options::{
    BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, BasicQosOptions,
    BasicRejectOptions, ExchangeDeclareOptions, QueueBindOptions, QueueDeclareOptions,
};
use lapin::types::FieldTable;
use lapin::{BasicProperties, Channel, Connection, ConnectionProperties, ExchangeKind};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{SharedState, TenantSpec};

pub struct Publisher {
    channel: Channel,
    exchange: String,
}

async fn open_channel(url: &str, exchange: &str) -> anyhow::Result<(Connection, Channel)> {
    let connection = Connection::connect(url, ConnectionProperties::default()).await?;
    let channel = connection.create_channel().await?;
    channel
        .exchange_declare(
            exchange,
            ExchangeKind::Topic,
            ExchangeDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    Ok((connection, channel))
}

impl Publisher {
    pub async fn connect(url: &str, exchange: &str) -> anyhow::Result<Self> {
        let (connection, channel) = open_channel(url, exchange).await?;
        // Keep the connection alive for the process lifetime.
        std::mem::forget(connection);
        tracing::info!("AMQP publisher ready (exchange: {exchange})");
        Ok(Self {
            channel,
            exchange: exchange.to_string(),
        })
    }

    pub async fn publish(&self, routing_key: &str, payload: &Value) -> anyhow::Result<()> {
        self.channel
            .basic_publish(
                &self.exchange,
                routing_key,
                BasicPublishOptions::default(),
                payload.to_string().as_bytes(),
                BasicProperties::default()
                    .with_content_type("application/json".into())
                    .with_delivery_mode(2),
            )
            .await?
            .await?;
        Ok(())
    }
}

#[derive(Deserialize)]
pub struct PublishBody {
    routing_key: String,
    payload: Value,
}

pub async fn publish(
    State(state): State<SharedState>,
    Json(body): Json<PublishBody>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let Some(publisher) = &state.amqp else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "AMQP is not configured" })),
        ));
    };
    publisher
        .publish(&body.routing_key, &body.payload)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": e.to_string() })),
            )
        })?;
    Ok(Json(json!({ "published": true })))
}

/// Start one consumer task per (tenant, routing key) pair across every
/// service in `GATEWAY_TENANTS` that declared any `routing_keys`.
pub async fn spawn_consumers(url: &str, exchange: &str, tenants: &[TenantSpec]) -> anyhow::Result<()> {
    let mut total = 0usize;
    for tenant in tenants {
        if tenant.routing_keys.is_empty() {
            continue;
        }
        let forward_url = tenant.forward_url.clone().ok_or_else(|| {
            anyhow::anyhow!("{}: forward_url is required when routing_keys is non-empty", tenant.service)
        })?;

        for routing_key in &tenant.routing_keys {
            let url = url.to_string();
            let exchange = exchange.to_string();
            let forward_url = forward_url.clone();
            let routing_key = routing_key.clone();
            // {service}.{routing_key} keeps queue names unique across every
            // tenant sharing this one exchange, matching the old
            // AMQP_QUEUE_PREFIX convention (previously one prefix per
            // gateway deployment; now one per tenant in the same process).
            let queue = format!("{}.{routing_key}", tenant.service);
            total += 1;
            tokio::spawn(async move {
                loop {
                    match consume_loop(&url, &exchange, &queue, &routing_key, &forward_url).await {
                        Ok(()) => tracing::warn!("consumer {queue} stream ended; reconnecting"),
                        Err(e) => tracing::error!("consumer {queue} failed: {e}; reconnecting"),
                    }
                    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                }
            });
        }
    }
    if total == 0 {
        tracing::info!("no AMQP consumers configured across any tenant");
    } else {
        tracing::info!("spawned {total} AMQP consumers across {} tenants", tenants.len());
    }
    Ok(())
}

async fn consume_loop(
    url: &str,
    exchange: &str,
    queue: &str,
    routing_key: &str,
    forward_url: &str,
) -> anyhow::Result<()> {
    let (_connection, channel) = open_channel(url, exchange).await?;
    channel
        .queue_declare(
            queue,
            QueueDeclareOptions {
                durable: true,
                ..Default::default()
            },
            FieldTable::default(),
        )
        .await?;
    channel
        .queue_bind(queue, exchange, routing_key, QueueBindOptions::default(), FieldTable::default())
        .await?;
    // Mirrors the BoundedSemaphore(10) worker cap in the Python consumer.
    channel.basic_qos(10, BasicQosOptions::default()).await?;

    let mut consumer = channel
        .basic_consume(
            queue,
            &format!("octy-data-gateway-{queue}"),
            BasicConsumeOptions::default(),
            FieldTable::default(),
        )
        .await?;
    tracing::info!("consuming {queue} (routing key {routing_key})");

    let http = reqwest::Client::new();
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery?;
        let payload: Value = match serde_json::from_slice(&delivery.data) {
            Ok(value) => value,
            Err(_) => {
                // Non-JSON payloads were refused without requeue in Python.
                tracing::error!("refused non-JSON message on {queue}");
                delivery
                    .reject(BasicRejectOptions { requeue: false })
                    .await
                    .ok();
                continue;
            }
        };

        let response = http
            .post(forward_url)
            .json(&json!({ "routing_key": routing_key, "payload": payload }))
            .send()
            .await;

        match response {
            Ok(resp) if resp.status().is_success() => {
                delivery.ack(BasicAckOptions::default()).await.ok();
            }
            Ok(resp) if resp.status().is_client_error() => {
                tracing::error!("component refused message on {queue}: {}", resp.status());
                delivery
                    .reject(BasicRejectOptions { requeue: false })
                    .await
                    .ok();
            }
            Ok(resp) => {
                tracing::error!("component errored on {queue}: {}; requeueing", resp.status());
                delivery
                    .reject(BasicRejectOptions { requeue: true })
                    .await
                    .ok();
            }
            Err(e) => {
                tracing::error!("forward to component failed: {e}; requeueing");
                delivery
                    .reject(BasicRejectOptions { requeue: true })
                    .await
                    .ok();
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
            }
        }
    }
    Ok(())
}
