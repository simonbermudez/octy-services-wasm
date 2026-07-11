//! Octy response envelopes, header validation, and pagination — shared ports
//! of each service's `api/routers/utils.py` and error-handler JSON bodies.

use octy_shared::errors::{ErrorReason, OctyError};
use serde_json::{json, Value};
use spin_sdk::http::{Request, Response};

pub fn json_response(status: u16, body: &Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(serde_json::to_vec(body).expect("serializable json"))
        .build()
}

pub fn error_response(err: &OctyError) -> Response {
    if err.code >= 500 {
        // Detail goes to the component log; clients get the same generic body
        // the FastAPI 500 handler produced.
        eprintln!("[octy-service] {err}: {:?}", err.reasons);
        let generic = OctyError::new(
            err.code,
            "Internal Server Error",
            vec![ErrorReason::new(
                "Unexpected error occurred when attempting to process this request",
                "",
            )],
        );
        return json_response(err.code, &generic.to_body());
    }
    json_response(err.code, &err.to_body())
}

/// FastAPI's default 404 handler, wrapped in the Octy envelope.
pub fn not_found() -> Response {
    let err = OctyError::new(
        404,
        "The requested URL was not found on the server. If you entered the URL manually please check your spelling and try again.",
        vec![],
    );
    json_response(404, &err.to_body())
}

pub fn method_not_allowed() -> Response {
    let err = OctyError::new(405, "The method is not allowed for the requested URL", vec![]);
    let mut body = err.to_body();
    body["request_meta"]["message"] = json!("Method Not Allowed");
    json_response(405, &body)
}

pub fn header_str<'r>(req: &'r Request, name: &str) -> Option<&'r str> {
    req.header(name).and_then(|v| v.as_str())
}

/// Port of `validate_post_headers` (identical across all services).
pub fn validate_post_headers(req: &Request, errors_help: &str) -> Result<(), OctyError> {
    // NB: exact match, like the Python (`!= 'application/json'`).
    let content_type = header_str(req, "content-type").unwrap_or("");
    if content_type != "application/json" {
        return Err(OctyError::new(
            400,
            "Missing header",
            vec![ErrorReason::new(
                "[Content-Type] : [application/json] header must be provided in request headers.",
                errors_help,
            )],
        ));
    }

    let content_length = header_str(req, "content-length").unwrap_or("");
    if content_length.is_empty() {
        return Err(OctyError::new(
            411,
            "Invalid headers provided",
            vec![ErrorReason::new(
                "[Content-Length] header must be provided in request headers.",
                "",
            )],
        ));
    }

    if header_str(req, "http-transfer-encoding").is_some() {
        return Err(OctyError::new(
            501,
            "Invalid headers provided",
            vec![ErrorReason::new(
                "[Transfer-Encoding] header must NOT be provided in request headers as it is not supported.",
                "",
            )],
        ));
    }

    Ok(())
}

/// Port of `validate_pagination_request`: when `identifier` is `None` a
/// `cursor` header is required; when an identifier query param is present the
/// cursor is not consulted (returns `Ok(None)`).
pub fn validate_pagination_request(
    req: &Request,
    identifier: Option<&str>,
) -> Result<Option<i64>, String> {
    if identifier.is_some() {
        return Ok(None);
    }
    match header_str(req, "cursor") {
        Some(raw) => raw
            .trim()
            .parse::<i64>()
            .map(Some)
            .map_err(|_| {
                "The value provided for the pagination header (-H cursor: str) could not be casted to type int."
                    .to_string()
            }),
        None => Err(
            "Please provide a valid object identifier within the query string eg: (?id=) or set a pagination header (-H cursor: str)"
                .to_string(),
        ),
    }
}

/// Best-effort client address (Spin exposes it as a request header).
pub fn client_addr(req: &Request) -> (String, i64) {
    let raw = header_str(req, "spin-client-addr").unwrap_or("unknown:0");
    match raw.rsplit_once(':') {
        Some((host, port)) => (host.to_string(), port.parse().unwrap_or(0)),
        None => (raw.to_string(), 0),
    }
}

/// Parse a query string parameter from the request URI (percent-decoded).
pub fn query_param(req: &Request, name: &str) -> Option<String> {
    let uri = req.uri();
    let query = uri.split_once('?').map(|(_, q)| q)?;
    url::form_urlencoded::parse(query.as_bytes())
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.into_owned())
}
