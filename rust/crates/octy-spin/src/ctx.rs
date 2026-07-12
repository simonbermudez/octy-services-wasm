//! Per-request application context, generalized for every Octy service.
//! Each Python service loaded `{SERVICE}_CONFIG` / `{SERVICE}_SECRETS`
//! base64-JSON blobs; the Spin components keep that contract via variables
//! (`{service}_config`) with an env-var fallback.

use octy_shared::config::Config;
use octy_shared::errors::OctyError;
use octy_shared::secrets::Secrets;

use crate::gateway::GatewayClient;

pub struct Ctx {
    pub config: Config,
    pub secrets: Secrets,
    pub gateway: GatewayClient,
}

/// Spin variable with env-var fallback (variables in deployments, plain env
/// when running components under bare `spin up` with `--env`).
pub fn variable(name: &str, env_fallback: &str) -> Result<String, OctyError> {
    spin_sdk::variables::get(name)
        .or_else(|_| std::env::var(env_fallback))
        .map_err(|_| OctyError::internal(format!("missing variable {name} / env {env_fallback}")))
}

impl Ctx {
    /// `prefix` is the service name, lowercase (e.g. `"events"` reads the
    /// `events_config` variable / `EVENTS_CONFIG` env var).
    pub fn load(prefix: &str) -> Result<Self, OctyError> {
        let upper = prefix.to_uppercase();
        let config = Config::from_encoded(&variable(
            &format!("{prefix}_config"),
            &format!("{upper}_CONFIG"),
        )?)?;
        let secrets = Secrets::from_encoded(&variable(
            &format!("{prefix}_secrets"),
            &format!("{upper}_SECRETS"),
        )?)?;
        let gateway = GatewayClient::new(
            variable("gateway_url", "GATEWAY_URL")
                .unwrap_or_else(|_| "http://127.0.0.1:8090".to_string()),
            prefix.to_string(),
        );
        Ok(Self {
            config,
            secrets,
            gateway,
        })
    }

    /// Redis connection string replicating the Python `db_redis_connect`
    /// (TLS + password from secrets); `db` is the service's database index.
    ///
    /// Production Redis (managed, e.g. DigitalOcean) presents a
    /// publicly-trusted certificate, so this always uses `rediss://`. Local
    /// development Redis typically runs with a self-signed certificate,
    /// which a normal TLS handshake will reject. Setting the
    /// `redis_insecure_tls` variable (`REDIS_INSECURE_TLS` env fallback) to
    /// `true`/`1` appends `#insecure` to the URL — a `redis-rs` extension
    /// that skips certificate/hostname verification while keeping the
    /// connection encrypted. Leave unset in every real deployment; this
    /// exists solely so a local minikube testbed can use a same-cluster
    /// Redis without provisioning a CA trusted by the node's TLS stack. See
    /// rust/local-dev/README.md.
    pub fn redis_address(&self, db: u32) -> Result<String, OctyError> {
        let host = self.config.get_str("REDIS_PUB_HOST")?;
        let port = self.config.get_i64("REDIS_PORT")?;
        let pass = self.secrets.get_str("REDIS_PASS")?;
        let insecure = variable("redis_insecure_tls", "REDIS_INSECURE_TLS")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);
        let suffix = if insecure { "#insecure" } else { "" };
        Ok(format!("rediss://:{pass}@{host}:{port}/{db}{suffix}"))
    }
}
