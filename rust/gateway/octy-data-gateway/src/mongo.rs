//! Generic MongoDB collection operations exposed over HTTP.
//! Documents/filters travel as legacy extended JSON (see `ejson`).
//!
//! Multi-tenant: every request identifies its caller via the `X-Octy-Service`
//! header (set by `octy_spin::gateway::GatewayClient`), which selects that
//! service's own `mongodb::Database` from `AppState::tenants`.

use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::Json;
use futures_util::TryStreamExt;
use mongodb::bson::Document;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::ejson::{bson_to_json, document_to_json, json_to_document};
use crate::SharedState;

type ApiResult = Result<Json<Value>, (StatusCode, Json<Value>)>;

fn map_error(err: mongodb::error::Error) -> (StatusCode, Json<Value>) {
    let message = err.to_string();
    // Surface duplicate-key violations distinctly (the account service maps
    // them to the Python 'Duplicate entry' 400).
    let status = if message.contains("E11000") || message.contains("duplicate key") {
        StatusCode::CONFLICT
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    (status, Json(json!({ "error": message })))
}

#[derive(Deserialize)]
pub struct FilterBody {
    #[serde(default)]
    filter: Value,
    #[serde(default)]
    skip: Option<i64>,
    #[serde(default)]
    limit: Option<i64>,
    /// Sort spec: `[["field", 1|-1], ...]` (order-preserving) or `{"field": 1|-1}`.
    #[serde(default)]
    sort: Option<Value>,
}

#[derive(Deserialize)]
pub struct InsertBody {
    document: Value,
}

#[derive(Deserialize)]
pub struct InsertManyBody {
    #[serde(default)]
    documents: Vec<Value>,
    #[serde(default)]
    ordered: Option<bool>,
}

#[derive(Deserialize)]
pub struct UpdateBody {
    #[serde(default)]
    filter: Value,
    update: Value,
}

#[derive(Deserialize)]
pub struct AggregateBody {
    #[serde(default)]
    pipeline: Vec<Value>,
}

/// Build an order-preserving sort document from either the array-of-pairs or
/// object form.
fn sort_document(sort: &Value) -> Document {
    let mut doc = Document::new();
    match sort {
        Value::Array(pairs) => {
            for pair in pairs {
                if let (Some(field), Some(dir)) = (
                    pair.get(0).and_then(Value::as_str),
                    pair.get(1).and_then(Value::as_i64),
                ) {
                    doc.insert(field, if dir < 0 { -1i32 } else { 1i32 });
                }
            }
        }
        Value::Object(map) => {
            for (field, dir) in map {
                doc.insert(field, if dir.as_i64().unwrap_or(1) < 0 { -1i32 } else { 1i32 });
            }
        }
        _ => {}
    }
    doc
}

fn service_name(headers: &HeaderMap) -> Result<&str, (StatusCode, Json<Value>)> {
    headers
        .get("x-octy-service")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            (
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "missing X-Octy-Service header" })),
            )
        })
}

fn collection(
    state: &SharedState,
    headers: &HeaderMap,
    name: &str,
) -> Result<mongodb::Collection<Document>, (StatusCode, Json<Value>)> {
    let service = service_name(headers)?;
    let tenant = state.tenants.get(service).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": format!("unknown service '{service}' (not in GATEWAY_TENANTS)") })),
        )
    })?;
    let db = tenant.db.as_ref().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": format!("service '{service}' has no db_uri configured") })),
        )
    })?;
    Ok(db.collection::<Document>(name))
}

pub async fn find_one(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<FilterBody>,
) -> ApiResult {
    let doc = collection(&state, &headers, &name)?
        .find_one(json_to_document(&body.filter))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({ "document": doc.map(|d| document_to_json(&d)) })))
}

pub async fn find(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<FilterBody>,
) -> ApiResult {
    let coll = collection(&state, &headers, &name)?;
    let mut query = coll.find(json_to_document(&body.filter));
    if let Some(skip) = body.skip.filter(|s| *s > 0) {
        query = query.skip(skip as u64);
    }
    if let Some(limit) = body.limit.filter(|l| *l > 0) {
        query = query.limit(limit);
    }
    if let Some(sort) = &body.sort {
        let sort_doc = sort_document(sort);
        if !sort_doc.is_empty() {
            query = query.sort(sort_doc);
        }
    }
    let docs: Vec<Document> = query
        .await
        .map_err(map_error)?
        .try_collect()
        .await
        .map_err(map_error)?;
    let documents: Vec<Value> = docs.iter().map(document_to_json).collect();
    Ok(Json(json!({ "documents": documents })))
}

pub async fn count(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<FilterBody>,
) -> ApiResult {
    let count = collection(&state, &headers, &name)?
        .count_documents(json_to_document(&body.filter))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({ "count": count })))
}

pub async fn insert_one(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<InsertBody>,
) -> ApiResult {
    let result = collection(&state, &headers, &name)?
        .insert_one(json_to_document(&body.document))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({ "inserted_id": bson_to_json(&result.inserted_id) })))
}

pub async fn update_one(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateBody>,
) -> ApiResult {
    let result = collection(&state, &headers, &name)?
        .update_one(json_to_document(&body.filter), json_to_document(&body.update))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({
        "matched": result.matched_count,
        "modified": result.modified_count,
    })))
}

pub async fn delete_one(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<FilterBody>,
) -> ApiResult {
    let result = collection(&state, &headers, &name)?
        .delete_one(json_to_document(&body.filter))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({ "deleted": result.deleted_count })))
}

pub async fn insert_many(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<InsertManyBody>,
) -> ApiResult {
    if body.documents.is_empty() {
        return Ok(Json(json!({ "inserted_ids": [], "inserted_count": 0 })));
    }
    let docs: Vec<Document> = body.documents.iter().map(json_to_document).collect();
    let result = collection(&state, &headers, &name)?
        .insert_many(docs)
        .ordered(body.ordered.unwrap_or(true))
        .await
        .map_err(map_error)?;
    // inserted_ids is keyed by document index; return them in input order
    let mut indexed: Vec<(&usize, &mongodb::bson::Bson)> = result.inserted_ids.iter().collect();
    indexed.sort_by_key(|(index, _)| **index);
    let inserted_ids: Vec<Value> = indexed.into_iter().map(|(_, id)| bson_to_json(id)).collect();
    Ok(Json(json!({
        "inserted_ids": inserted_ids,
        "inserted_count": result.inserted_ids.len(),
    })))
}

pub async fn update_many(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<UpdateBody>,
) -> ApiResult {
    let result = collection(&state, &headers, &name)?
        .update_many(json_to_document(&body.filter), json_to_document(&body.update))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({
        "matched": result.matched_count,
        "modified": result.modified_count,
        "matched_count": result.matched_count,
        "modified_count": result.modified_count,
    })))
}

pub async fn delete_many(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<FilterBody>,
) -> ApiResult {
    let result = collection(&state, &headers, &name)?
        .delete_many(json_to_document(&body.filter))
        .await
        .map_err(map_error)?;
    Ok(Json(json!({
        "deleted": result.deleted_count,
        "deleted_count": result.deleted_count,
    })))
}

pub async fn aggregate(
    State(state): State<SharedState>,
    Path(name): Path<String>,
    headers: HeaderMap,
    Json(body): Json<AggregateBody>,
) -> ApiResult {
    let pipeline: Vec<Document> = body.pipeline.iter().map(json_to_document).collect();
    let docs: Vec<Document> = collection(&state, &headers, &name)?
        .aggregate(pipeline)
        .await
        .map_err(map_error)?
        .try_collect()
        .await
        .map_err(map_error)?;
    let documents: Vec<Value> = docs.iter().map(document_to_json).collect();
    Ok(Json(json!({ "documents": documents })))
}
