# Octy services — Rust / WASM rewrite

Full Rust port of the Octy Python microservices (12 FastAPI services + 5 ML
workers, ~33k lines of Python), compiled to WebAssembly and run as **WASM
containers** on Kubernetes ([SpinKube](https://www.spinkube.dev) /
`containerd-shim-spin`).

Status: **all 17 services/workers ported**, workspace builds clean to
`wasm32-wasip1` (zero errors, one harmless dead-code warning), 51 native unit
tests pass, and `account`/`billing`/`admin` have been smoke-tested end-to-end
under `spin up`.

**Setting up a local testbed?** See [`local-dev/README.md`](local-dev/README.md) —
minikube + local MongoDB/Redis/RabbitMQ/MinIO, ready-made per-service
config/secrets files, and a fast `spin up` iteration loop.

## Layout

```
rust/
├── Cargo.toml                    # workspace (17 wasm32 members + gateway)
├── crates/
│   ├── octy-shared/               # portable domain logic — native AND wasm32
│   │   ├── config.rs               #   base64-JSON config/secrets blobs (unchanged contract)
│   │   ├── errors.rs               #   Octy error envelope (byte-compatible with FastAPI handlers)
│   │   ├── jwt.rs                  #   RS256 fat-JWT sign + verify (pure-Rust rsa/sha2)
│   │   ├── sigv4.rs                #   AWS SigV4 request signing (pure-Rust, verified against AWS's own worked example)
│   │   ├── models.rs               #   request models + pydantic-equivalent validation
│   │   ├── ejson.rs                #   bson.json_util legacy extended JSON helpers
│   │   └── utils.rs                #   uid generation, basic-auth parsing, dt encoding …
│   └── octy-spin/                 # shared Spin (wasm32-only) plumbing
│       ├── ctx.rs                  #   Ctx::load(prefix) — config/secrets/gateway per service
│       ├── gateway.rs               #   HTTP client for octy-data-gateway
│       ├── http_util.rs            #   Octy response envelopes, header/pagination validation
│       ├── auth.rs                 #   decode_account_jwt — X-AUTH-JWT verification + REQUIRED_PERMISSIONS
│       └── aws.rs                  #   SigV4-signed outbound requests (SageMaker, S3 data-plane)
├── services/                      # 12 Spin HTTP components (wasm32-wasip1)
│   ├── account/  admin/  billing/  churn-prediction/  configurations/
│   ├── events/   items/  messaging/  octy-jobs/  profiles/
│   └── recommendation/  segmentation/
├── workers/                       # 5 Spin HTTP components — AMQP-triggered ML/batch jobs
│   ├── churn-prediction/  profile-identification/  recommendation/
│   └── rfm/  segmentation/
├── gateway/
│   └── octy-data-gateway/         # native sidecar: MongoDB + RabbitMQ + S3 bridge
└── kubernetes/<service>/          # SpinApp + gateway manifests, one dir per service
```

Every service/worker crate follows the same internal shape (established by
`services/account`): `lib.rs` (router + 405 table), `handlers.rs`, `models.rs`
(pydantic-equivalent validation), `http_util.rs` or reuse of
`octy_spin::http_util` (DTOs), `repos/` (Mongo/Redis/S3 access), `services/`
(business logic), `amqp.rs` (`POST /internal/amqp/consume` for AMQP-driven
services).

## Architecture: why a data gateway?

WASM components have no raw TCP sockets, so the native MongoDB/AMQP drivers
cannot run inside them. Per service we deploy:

* **The service component** (`wasm32-wasip1`, Spin) — all business logic,
  HTTP routes, validation, authn/z, JWT minting, Redis (via Spin's
  outbound-Redis host support), and outbound HTTPS (Mailjet, SageMaker via
  SigV4, internal service-to-service calls).
* **`octy-data-gateway`** (native container, one deployment per service) — a
  thin, service-agnostic bridge exposing:
  * **MongoDB**: `POST /v1/mongo/{collection}/{find-one, find, count,
    insert-one, insert-many, update-one, update-many, delete-one,
    delete-many, aggregate}` — documents travel as `bson.json_util`-style
    legacy extended JSON (`{"$oid": …}`, `{"$date": <millis>}`); `find`
    accepts an optional `sort` (array-of-pairs or object form).
  * **AMQP**: `POST /v1/amqp/publish`; consumers declare a queue per
    configured routing key and forward each delivery to the component as
    `POST /internal/amqp/consume` — the component's HTTP response drives
    ack/reject (2xx ack, 4xx reject-no-requeue, 5xx reject+requeue).
  * **S3**: `POST /v1/s3/{create-bucket, configure-bucket, create-directory,
    delete-bucket, put-object, get-object, list-objects, delete-object}`
    (object bodies travel base64-encoded).

Redis, Mailjet, SageMaker (via SigV4), and service-to-service HTTP go
**directly** from the WASM component; only Mongo/AMQP/S3 — backends that need
a real TCP driver or the AWS SDK — go through the gateway. The gateway image
is identical across all 17 services; only its environment (DB_URI, AMQP
consumer list, AWS creds) differs per deployment.

### Compatibility contracts kept from the Python services

* Same routes, request/response JSON bodies, and error envelopes.
* Same `{SERVICE}_CONFIG` / `{SERVICE}_SECRETS` base64-JSON blobs (env var or
  Spin variable, e.g. `ACCOUNT_CONFIG`/`account_config`).
* Same Redis cache keys/db indices and legacy extended JSON encoding, so Rust
  and Python services can run side by side against the same Mongo/Redis
  during a staged migration.
* Same Argon2id parameters as argon2-cffi's defaults — secret keys hashed by
  either implementation verify in the other.
* Same RS256 fat-JWT claims (including the `YYYYMMDDHHMMSS` int `iat`/`exp`).
* Same AMQP routing keys, exchange topology, and queue semantics.

### Production bugs found in the Python codebase during the port

The porting agents were instructed to preserve Python behavior bug-for-bug
*unless clearly broken*, which surfaced a number of real defects — some were
fixed as "obviously intended" behavior, others preserved verbatim and flagged.
Worth triaging against the live Python services:

| Service | Bug | Disposition |
|---|---|---|
| `events` | Every event-type endpoint called its Motor repository without `await` → `TypeError` on every request → **always 500'd in production** | Fixed (awaited as intended) |
| `events` | Custom event types were never deleted on account cleanup (missing `await`) | Fixed |
| `octy_jobs` | New job documents were missing `status`/timestamps the scheduler's own filter required → jobs likely never picked up | Fixed |
| `octy_jobs` | Account-deletion AMQP publish used the queue name as the routing key instead of the real key | Preserved (flagged) |
| `segmentation` (service) | Three call sites invoke async repo methods without `await` → 500 in production | Fixed (awaited as intended) |
| `segmentation` (worker) | `get_events`'s profile-id filter is inverted — a non-empty filter is *dropped*, so `PendingLiveSegmentation` silently scans the whole account instead of one profile | Preserved (flagged — real correctness bug worth fixing upstream) |
| `messaging` | `update_templates` scopes its Mongo filter by the fetched doc's own `account_id`, not the caller's — **any account can overwrite another account's template by id (IDOR)** | Preserved per brief — **recommend fixing in Python too** |
| `messaging` | Rybbon campaign-pagination loop used a Python chained comparison (`200 > x < 500`) that's always `False` — infinite loop on every call | Fixed (would have hung a WASM request forever) |
| `churn_prediction` (worker) | Job deletion (`_destroy_job`) crashes before publishing the actual delete message — cleanup never completes | Preserved (flagged) |
| `account` | `_log_failed_auth` called `len()` on an un-awaited coroutine — the security-warning email never sent | Fixed |
| `admin` | Webhook handler awaited a sync Redis call — every successful GitHub webhook delivery 500'd *after* caching | Fixed |
| `profiles` | `get_profiles_meta` crashes (`KeyError`) on any profile without an `updated_at` (i.e. never-updated profiles, the common case) | Fixed |
| `profiles` | Bulk delete published exactly one cascade-delete event using a leaked loop variable — dropped the event for every profile but the last | Fixed |

Full per-service detail (including intentionally-preserved dead code,
formatting quirks, and rounding differences) lives in comments at the
relevant call sites.

### Known divergences (intentional, by design)

* **Rate limits** (slowapi) must be enforced at the ingress — Spin components
  are stateless per request.
* **Sentry** is not wired up; component stderr goes to the Spin/containerd
  logs. Add an OTLP collector if needed.
* Retry fan-out (`requests_retry_session`) retries immediately instead of
  with backoff — no timer host call is available inside a component.
* **`octy_jobs`'s in-process scheduler** (a 2-minute polling loop) cannot run
  inside a per-request WASM component. It became `POST
  /internal/scheduler/tick`, driven by a Kubernetes `CronJob`
  (`kubernetes/octy-jobs/scheduler-cronjob.yml`, `*/2 * * * *`,
  `concurrencyPolicy: Forbid`).
* **ML model artifacts**: the `rfm`, `churn-prediction`, and `recommendation`
  workers persisted trained models via `joblib`/pickle — unreadable from
  Rust. The SageMaker training images must be updated to additionally emit a
  JSON sidecar (documented per-worker below); until then, the *complete*
  pipeline stage will error and reschedule the job (matching the Python's own
  bug-for-bug "any exception reschedules" behavior).
* **S3 multipart upload** is not implemented in the gateway; large training
  datasets upload via a single `put-object` instead of chunked MPU (the
  original chunk-count *validation* logic is preserved, just not the
  chunked transfer itself). Add `/v1/s3/{create,upload-part,complete,abort}-multipart-upload`
  if dataset sizes exceed what a single PUT can carry.

## Post-port hardening pass

After the initial port, every crate went through a second pass to backport
non-obvious business-rule comments from the Python source (comment-only —
verified with `git diff` per crate to confirm no logic changed) and to fix
issues that surfaced while researching the [local dev guide](local-dev/README.md):

* **AMQP exchange name mismatch (fixed)** — 6 of 17 gateway manifests
  declared `AMQP_EXCHANGE: "octy"` while the other 11 declared
  `"octy-services"`. RabbitMQ topic exchanges are independent namespaces; a
  publisher and consumer on different exchanges never see each other's
  messages, silently. Standardized all 17 manifests to `"octy-services"`
  ([kubernetes/*/data-gateway.yml](kubernetes/)) — **verify this matches your
  real deployment's `Config['EXCHANGE']` value before deploying**, since it
  couldn't be confirmed against a live production config blob during this
  pass.
* **`octy_public_key` empty-default fallback (fixed)** — 5 of 6
  JWT-verifying services declared this Spin variable with `default = ""`
  rather than `required = true`. An unset variable then resolves to `""`
  (a successful lookup, not an error), which skipped straight past the
  intended fallback to the packaged `keys/octy-public-key.pub` file and
  turned every authenticated request into a PEM-parse 500. Fixed once in
  [`octy_spin::auth::load_public_key_pem`](crates/octy-spin/src/auth.rs) —
  a blank variable value is now treated as unset.
* **MinIO/S3-compatible endpoint support (added)** — `octy-data-gateway` now
  respects `AWS_ENDPOINT_URL` (standard `aws-config` env resolution) and a
  new `S3_FORCE_PATH_STYLE` env var, purely additive and inert unless set —
  see [`gateway/octy-data-gateway/src/s3.rs`](gateway/octy-data-gateway/src/s3.rs).
* **`redis_insecure_tls` escape hatch (added)** — see the comment on
  `redis_address()` in `ctx.rs` and the [local dev guide](local-dev/README.md)
  for why local Redis needs this and production must never set it.
* **Flagged, not fixed** (per the audit's scope — comment-only unless
  explicitly asked to fix): `segmentation`-service's Mongo write-failure path
  never actually produces the `"[toxic]::"` marker its own AMQP consumer
  checks for non-retryable rejection (so those failures currently requeue
  instead of dead-lettering); `profile-identification-worker` sends AMQP/
  webhook payloads as a single message with no size-based chunking, unlike
  the Python original's ~100MB split. Both are marked `// NOTE:` at the
  relevant code.

## The ML workers

Three of the five workers orchestrate SageMaker training (control-plane calls
via `octy_spin::aws::send_signed`, SigV4 service `"sagemaker"`) and do **not**
reimplement gradient boosting / matrix factorization in Rust — that stays in
the existing SageMaker training container images, unchanged:

| Worker | Training | Inference | Notes |
|---|---|---|---|
| `rfm-worker` | SageMaker (opaque container) | N/A (scores come from the container) | The Python's `sklearn.cluster.KMeans`/`kneed` imports were **dead code** — never called. A hand-rolled, unit-tested k-means++/Kneedle (`ml.rs`) exists for a possible future in-process path but isn't wired into the live pipeline. |
| `churn-prediction-worker` | SageMaker (XGBoost, Bayesian HPO) | **Local**, in the worker | `xgb.rs` is a from-scratch gbtree/`binary:logistic` scorer reading XGBoost's native JSON model dump (walks trees, sums leaf values, sigmoid) — no SageMaker runtime endpoint call needed. |
| `recommendation-worker` | SageMaker (LightFM container) | **Local**, in the worker | Downloads trained embeddings/biases and computes `user·item + biases` dot products directly. |
| `profile-identification-worker` | N/A | N/A | Pure identity-resolution/matching logic (`matching.rs`), no ML at all — despite the name, no clustering. |
| `segmentation-worker` | N/A | N/A | Pure rule-based event/property matching against Mongo + the profile/event services — no ML. |

**Artifact format changes** (breaking, requires updating the training
containers): `trained_*_model.pkl` → `trained_*_model.json`
(`Booster.save_model(format="json")` for XGBoost; a `{users_biases,
users_embeddings, items_biases, items_embeddings}` JSON for LightFM),
`features.pkl`/`df_scores.pkl` → `features.json`/`df_scores.json`. Until the
training images emit these, the completion stage of each pipeline will error.

## Build & test

```bash
cd rust

# native: shared crates + gateway
cargo build
cargo test          # 18 tests, octy-shared

# every wasm32 service + worker (17 crates)
cargo build --workspace --exclude octy-data-gateway --target wasm32-wasip1 --release

# a single service
cargo build -p account-service --target wasm32-wasip1 --release

# native unit tests inside a worker's pure-logic modules (frame.rs, ml.rs, xgb.rs, matching.rs, …)
# — these compile for the host target too, since they have no spin-sdk imports
cargo test -p rfm-worker --lib
cargo test -p churn-prediction-worker --lib
```

Run a service locally (needs the [Spin CLI](https://spinframework.dev)):

```bash
export SPIN_VARIABLE_ACCOUNT_CONFIG=$(base64 < account-config.json)
export SPIN_VARIABLE_ACCOUNT_SECRETS=$(base64 < account-secrets.json)
export SPIN_VARIABLE_OCTY_PRIVATE_KEY=$(base64 < octy-private-key.pem)
export SPIN_VARIABLE_GATEWAY_URL=http://127.0.0.1:8090

# the gateway (against real Mongo/RabbitMQ/S3):
DB_URI=… AMQP_URL=… AMQP_EXCHANGE=octy \
  AMQP_CONSUMERS='["account.configs.cmd.update","algo.configs.cmd.update","churn.info.cmd.update"]' \
  AMQP_FORWARD_URL=http://127.0.0.1:3000/internal/amqp/consume \
  cargo run -p octy-data-gateway

cd services/account && spin up
```

## Deploy (WASM containers on Kubernetes)

1. Install the SpinKube operator + `wasmtime-spin-v2` runtime class on the
   cluster.
2. `docker build -f gateway/octy-data-gateway/Dockerfile -t <registry>/octy-data-gateway:0.1.0 rust/`
3. For each service: `cd services/<name> && spin build && spin registry push <registry>/octy-<name>:0.1.0`
4. `kubectl apply -f kubernetes/<name>/`

Every `kubernetes/<name>/` directory has a `<name>-spinapp.yml` (SpinApp +
Service) and `data-gateway.yml` (a dedicated gateway Deployment with that
service's `AMQP_CONSUMERS`/`AMQP_QUEUE_PREFIX`/`AMQP_FORWARD_URL`, or no AMQP
env at all for services with no consumers). `octy-jobs` additionally has
`scheduler-cronjob.yml`.

## Extending the port

Adding a new route or service follows the pattern in `services/account`:
routers → `handlers.rs`; pydantic-style request models → a per-crate
`models.rs` (or `octy_shared::models` if genuinely shared); repositories →
`repos/` on top of `octy_spin::gateway::GatewayClient`; DTOs →
`octy_spin::http_util` re-exports; AMQP consumers → handled inside
`/internal/amqp/consume`, with routing keys added to that service's
`AMQP_CONSUMERS` gateway env.
