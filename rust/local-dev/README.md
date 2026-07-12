# Local development testbed

A from-scratch guide to running the Octy Rust/WASM services against local
MongoDB, Redis, and RabbitMQ in minikube. Two paths, pick based on what
you're doing:

- **[Path A — fast loop](#path-a-fast-loop-recommended)** (recommended for day-to-day work): minikube
  hosts only the stateful backends (Mongo/Redis/RabbitMQ/MinIO); each Spin
  service runs on your machine via `spin up`, hot-reloading on save. This is
  the path used to smoke-test `account`, `billing`, and `admin` during
  development — it's verified to work.
- **[Path B — full cluster](#path-b-full-cluster-spinkube-in-minikube)**: every service deploys into minikube as a
  real WASM container via SpinKube, matching production topology exactly.
  Slower iteration, but proves the actual deployment path. Do this before
  shipping a change to the Kubernetes manifests themselves.

Both paths share the same local infrastructure manifests and config/secrets
files under this directory.

## Prerequisites

```bash
# Rust + the wasm32-wasip1 target
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --target wasm32-wasip1

# Spin CLI (see https://spinframework.dev for other platforms)
curl -fsSL https://spinframework.dev/downloads/install.sh | bash
sudo mv spin /usr/local/bin/

# minikube + kubectl
brew install minikube kubectl   # macOS; see minikube.sigs.k8s.io for other OSes

# openssl (for the local JWT keypair and Redis TLS cert) — already on macOS
```

Start the cluster once:

```bash
minikube start --cpus=4 --memory=8192
```

## 1. Deploy local infrastructure

```bash
cd rust/local-dev
kubectl apply -f infra/namespace.yaml
kubectl apply -f infra/mongo.yaml
kubectl apply -f infra/rabbitmq.yaml
kubectl apply -f infra/redis.yaml
kubectl apply -f infra/minio.yaml   # optional — only needed for S3-touching code paths, see below

kubectl -n octy-local wait --for=condition=available --timeout=120s \
  deployment/mongo deployment/rabbitmq deployment/redis
```

This gives you, inside the cluster:

| Backend | In-cluster address | Local creds |
|---|---|---|
| MongoDB | `mongo.octy-local:27017` (db `octy`) | none |
| Redis (TLS, self-signed) | `redis.octy-local:6379` | password `devpassword` |
| RabbitMQ | `rabbitmq.octy-local:5672` | `octy` / `octy` |
| RabbitMQ management UI | `rabbitmq.octy-local:15672` | `octy` / `octy` |
| MinIO (S3-compatible, optional) | `minio.octy-local:9000` (console `:9001`) | `minioadmin` / `minioadmin` |

**Why Redis needs a self-signed TLS cert, and what `redis_insecure_tls` is
for**: every service builds its Redis connection string as `rediss://…`
unconditionally — this matches production's managed Redis, which requires
TLS. `infra/redis.yaml` generates a throwaway self-signed cert on pod start
so the local Redis also speaks TLS. Because that cert isn't signed by a CA
your machine trusts, any service talking to this Redis needs
`redis_insecure_tls: "true"` set (see [`redis_address()`](../crates/octy-spin/src/ctx.rs) —
this appends `#insecure` to the URL, a `redis-rs` extension that keeps the
connection encrypted but skips certificate verification). **Never set this
variable in a real deployment** — it exists solely so local dev doesn't
require provisioning a CA trusted by the cluster nodes. The services that use
Redis (and therefore need this variable set) are: `account`, `admin`,
`octy-jobs`, `profiles`, `profile-identification-worker`.

**Do you need MinIO?** Only if you're exercising S3-touching code: account
bucket provisioning, messaging template storage, or the ML workers'
dataset/model artifacts. The gateway degrades gracefully without it — S3
calls just return `ok: false` / `found: false` rather than crashing. Skip it
for a first pass.

## 2. Generate a local JWT keypair

The account service signs the "fat JWT" with an RSA private key; every other
authenticated service verifies it with the matching public key. Generate a
throwaway pair — **do not reuse a real key for local dev, and never commit
this to the repo**:

```bash
mkdir -p /tmp/octy-local-keys
openssl genpkey -algorithm RSA -pkeyopt rsa_keygen_bits:2048 \
  -out /tmp/octy-local-keys/private.pem
openssl rsa -pubout -in /tmp/octy-local-keys/private.pem \
  -out /tmp/octy-local-keys/public.pem
```

## Path A: fast loop (recommended)

Reach the in-cluster backends from your machine:

```bash
kubectl -n octy-local port-forward svc/mongo 27017:27017 &
kubectl -n octy-local port-forward svc/redis 6379:6379 &
kubectl -n octy-local port-forward svc/rabbitmq 5672:5672 &
kubectl -n octy-local port-forward svc/minio 9000:9000 &   # only if using MinIO
```

Run the gateway natively — **one shared instance serves every service**
(previously one per service; see `rust/README.md`'s architecture notes for
why). `gateway-tenants.json` in this directory already has every service's
`db_uri` pointed at the local Mongo above and its `forward_url` pointed at
`http://127.0.0.1:3000` — adjust the port per service if you're running more
than one at once (see the note at the end of this section):

```bash
cd rust
export PATH="$HOME/.cargo/bin:$PATH"
AMQP_URL="amqp://octy:octy@localhost:5672/%2f" \
  AMQP_EXCHANGE="octy-services" \
  GATEWAY_TENANTS="$(cat local-dev/gateway-tenants.json)" \
  AWS_REGION=us-east-1 AWS_ACCESS_KEY_ID=minioadmin AWS_SECRET_ACCESS_KEY=minioadmin \
  AWS_ENDPOINT_URL=http://localhost:9000 S3_FORCE_PATH_STYLE=true \
  cargo run -p octy-data-gateway
```

Run a service (`account` shown — repeat for any other service, see the
[per-service reference](#per-service-reference) table below for exact
variable names):

```bash
cd rust/services/account
export SPIN_VARIABLE_ACCOUNT_CONFIG=$(base64 < ../../local-dev/config/account.config.json)
export SPIN_VARIABLE_ACCOUNT_SECRETS=$(base64 < ../../local-dev/config/account.secrets.json)
export SPIN_VARIABLE_OCTY_PRIVATE_KEY=$(base64 < /tmp/octy-local-keys/private.pem)
export SPIN_VARIABLE_GATEWAY_URL=http://127.0.0.1:8090
spin up --listen 127.0.0.1:3000
```

Verify:

```bash
curl http://127.0.0.1:3000/healthz
# "OK"

curl -X POST http://127.0.0.1:3000/v1/admin/account/create \
  -H 'content-type: application/json' \
  -d '{
    "contact_email_address": "dev@octy.local",
    "account_name": "Local Test Account",
    "account_type": "startup",
    "account_currency": "GBP",
    "contact_name": "Dev",
    "contact_surname": "User",
    "webhook_url": "https://example.com/webhook",
    "permissions": ["rec", "churn"]
  }'
```

A `201` with `pk`/`sk` in the body means the whole chain works: the WASM
component validated the request, the gateway wrote to your local MongoDB and
published to your local RabbitMQ, and (if MinIO is running) provisioned a
bucket. Save the `pk`/`sk` and authenticate:

```bash
curl http://127.0.0.1:3000/v1/account/authenticate \
  -H "Authorization: Basic $(printf '%s:%s' '<pk>' '<sk>' | base64)"
```

A `200` with a JWT in `auth.jwt` (and the `X-AUTH-JWT` response header) means
Redis, Argon2 verification, and JWT signing all work. Use that JWT as the
`X-AUTH-JWT` header against any other service you bring up the same way —
copy `/tmp/octy-local-keys/public.pem` into that service's `keys/` directory
override, or set its `octy_public_key` variable directly (see the
per-service table).

To run a second service alongside `account`, just repeat the `spin up` step
in a new terminal with a different `--listen` port — the same gateway
instance already has every service's tenant entry loaded, no need to restart
it. The one thing to watch: `gateway-tenants.json`'s `forward_url`s assume
`account` is on port 3000; if you're running the service whose AMQP
consumers you actually want to exercise, update its `forward_url` in that
file to match the port you chose (see the [per-service reference](#per-service-reference)
table for which services have consumers at all — most don't, and can be
run on any port with no changes).

## Path B: full cluster (SpinKube in minikube)

This mirrors the real deployment path in [`../kubernetes/`](../kubernetes/)
exactly, using minikube as the target cluster. It requires an OCI registry
reachable both from your machine (to push) and from cluster nodes (to pull);
minikube's `registry` addon runs a `localhost:5000` proxy on every node
specifically so the same reference works in both places.

### Install SpinKube

```bash
minikube addons enable registry

kubectl apply -f https://github.com/spinframework/spin-operator/releases/latest/download/spin-operator.crds.yaml
kubectl apply -f https://github.com/spinframework/spin-operator/releases/latest/download/spin-operator.runtime-class.yaml

helm install spin-operator \
  --namespace spin-operator --create-namespace \
  --version 0.5.0 \
  oci://ghcr.io/spinframework/charts/spin-operator

kubectl -n spin-operator wait --for=condition=available --timeout=120s deployment/spin-operator-controller-manager
```

### Push images to the local registry

```bash
kubectl port-forward --namespace kube-system svc/registry 5000:80 &

# the gateway (a normal OCI container image)
cd rust
docker build -f gateway/octy-data-gateway/Dockerfile -t localhost:5000/octy-data-gateway:dev .
docker push localhost:5000/octy-data-gateway:dev

# a service (a Spin OCI artifact — different push path)
cd services/account
export PATH="$HOME/.cargo/bin:$PATH"
cargo build -p account-service --target wasm32-wasip1 --release
spin build
spin registry push localhost:5000/octy-account:dev
```

Repeat the `spin build && spin registry push` step for every service you
want running in-cluster, substituting its crate name and image tag.

### Deploy

Apply the local infra (§1 above) if you haven't, then the **one shared
gateway** (not per service):

```bash
kubectl apply -f rust/kubernetes/gateway/gateway.yml -n octy-local
```

Before applying, edit `gateway.yml`'s `octy-gateway-secrets`:
- Replace `<registry>/octy-data-gateway:0.1.0` with `localhost:5000/octy-data-gateway:dev`.
- Point `AMQP_URL` and every tenant's `db_uri` in `GATEWAY_TENANTS` at the
  `octy-local` namespace hostnames from the table in §1 — or just paste in
  [`gateway-tenants.json`](gateway-tenants.json) verbatim, it's already
  built for this local cluster.
- Point AWS env at MinIO if you're using it (see §1's MinIO note).

Then for each service:

```bash
kubectl apply -f rust/kubernetes/account/account-spinapp.yml -n octy-local
```

Before applying, edit each `*-spinapp.yml`:
- Replace `<registry>/octy-<name>:0.1.0` with `localhost:5000/octy-<name>:dev`.
- `gateway_url` already points at `http://octy-gateway:8090` (the shared
  gateway's Service name) — no change needed.
- Add `redis_insecure_tls: "true"` to the SpinApp's `variables` list for any
  service that uses Redis (see §1).

Generate the ConfigMap/Secret each SpinApp references with `encode.sh`
(exact object/key names are in the [per-service reference](#per-service-reference) table):

```bash
./encode.sh -f config/account.config.json -y /tmp/account-configmap.yaml \
  -k ACCOUNT_CONFIG -n account-config
kubectl apply -f /tmp/account-configmap.yaml

./encode.sh -f config/account.secrets.json -y /tmp/account-secrets.yaml \
  -k ACCOUNT_SECRETS -n account-secrets -s
kubectl apply -f /tmp/account-secrets.yaml

# account also needs OCTY_PRIVATE_KEY as a second key in the same Secret:
kubectl -n octy-local patch secret account-secrets --type merge -p \
  "{\"data\":{\"OCTY_PRIVATE_KEY\":\"$(base64 < /tmp/octy-local-keys/private.pem | tr -d '\n')\"}}"
```

Verify:

```bash
kubectl -n octy-local get spinapps
kubectl -n octy-local port-forward svc/account-service 8080:80
curl http://localhost:8080/healthz
```

## Per-service reference

Every service's exact ConfigMap/Secret object name and data key — these are
**not uniform** across services (a byproduct of 16 crates being ported
independently), so don't assume a pattern; use this table. `redis?` marks
services that need `redis_insecure_tls: "true"` in Path B. `amqp consumers`
lists the routing keys each service consumes — these live in the shared
gateway's `GATEWAY_TENANTS` now (see [`gateway-tenants.json`](gateway-tenants.json)),
not in per-service env.

| Service | Config object / key | Secrets object / key | redis? | amqp consumers |
|---|---|---|---|---|
| account | `account-config` / `ACCOUNT_CONFIG` | `account-secrets` / `ACCOUNT_SECRETS` (+`OCTY_PRIVATE_KEY`) | yes | `account.configs.cmd.update`, `algo.configs.cmd.update`, `churn.info.cmd.update` |
| admin *(also serves what was `configurations`)* | `admin-config` / `admin_config` **and** `configurations-config` / `CONFIGURATIONS_CONFIG` (+`OCTY_PUBLIC_KEY` in the same ConfigMap) | `admin-secrets` / `admin_secrets` **and** `configurations-secrets` / `CONFIGURATIONS_SECRETS` | yes (admin's own routes only) | — (configurations' routes only publish, never consume) |
| billing | `billing-config` / `billing_config` | `billing-secrets` / `billing_secrets` | no | `account.billing.cmd.capture` |
| churn-prediction | `churn-prediction-config` / `CHURN_PREDICTION_CONFIG` | `churn-prediction-secrets` / `CHURN_PREDICTION_SECRETS` (+`OCTY_PUBLIC_KEY`) | no | — |
| events | `events-config` / `EVENTS_CONFIG` | `events-secrets` / `EVENTS_SECRETS` (+`OCTY_PUBLIC_KEY`) | no | `events.cmd.delete`, `events.cmd.update` |
| items | `items-config` / `ITEMS_CONFIG` | `items-secrets` / `ITEMS_SECRETS` (+`OCTY_PUBLIC_KEY`) | no | — |
| messaging | `messaging-config` / `MESSAGING_CONFIG` | `messaging-secrets` / `MESSAGING_SECRETS` (+`OCTY_PUBLIC_KEY`) | no | — |
| octy-jobs | `octy-jobs-config` / `octy_jobs_config` | `octy-jobs-secrets` / `octy_jobs_secrets` | yes | `octy.job.cmd.create`, `octy.job.cmd.delete` |
| profiles | `profiles-config` / `PROFILES_CONFIG` | `profiles-secrets` / `PROFILES_SECRETS` (+`OCTY_PUBLIC_KEY`) | yes | `profiles.cmd.update`, `segment.tags.cmd.update.delete`, `grouped.segmentation.operations.cmd` |
| recommendation | `recommendation-config` / `RECOMMENDATION_CONFIG` | `recommendation-secrets` / `RECOMMENDATION_SECRETS` | no | `reccache.cmd.delete` |
| segmentation | `segmentation-config` / `SEGMENTATION_CONFIG` | `segmentation-secrets` / `SEGMENTATION_SECRETS` (+`OCTY_PUBLIC_KEY`) | no | `segment.profiles.cmd.update` |
| rfm-worker | `rfm-worker-config` / `rfm_worker_config` | `rfm-worker-secrets` / `rfm_worker_secrets` | no | `rfm.training.cmd.run`, `rfm.training.complete.cmd.run` |
| churn-prediction-worker | `churn-prediction-worker-config` / `churn_prediction_worker_config` | `churn-prediction-worker-secrets` / `churn_prediction_worker_secrets` | no | `churn.training.cmd.run`, `churn.training.complete.cmd.run` |
| recommendation-worker | `recommendation-worker-config` / `recommendation_worker_config` | `recommendation-worker-secrets` / `recommendation_worker_secrets` | no | `rec.training.cmd.run`, `rec.training.complete.cmd.run` |
| segmentation-worker | `segmentation-worker-config` / `segmentation_worker_config` | `segmentation-worker-secrets` / `segmentation_worker_secrets` | no | `past.segmentation.cmd.run`, `live.segmentation.cmd.run` |
| profile-identification-worker | `profile-identification-worker-config` / `profile_identification_worker_config` | `profile-identification-worker-secrets` / `profile_identification_worker_secrets` | yes | `profile.identification.cmd.run` |

Note the `service` names above (used as the table's row labels, matching
directory/crate names) mostly match what each crate passes to `Ctx::load(…)` —
**except `octy-jobs`, whose code calls `Ctx::load("octy_job")` (singular)**.
That singular form is what must appear as the `"service"` field in
`gateway-tenants.json`/`GATEWAY_TENANTS` (it's the literal `X-Octy-Service`
header value the gateway matches on) — grep `Ctx::load(` in a crate if
you're ever unsure which string to use.

Every service also has a filled-in `config/<name>.config.json` and, where
applicable, `<name>.secrets.json` in this directory — they use the local
infra hostnames from §1 and are ready to `base64` and load directly (for
Path A) or feed to `encode.sh` (for Path B). Services with `OCTY_PUBLIC_KEY`
in the table above need it as a Spin variable (Path A:
`SPIN_VARIABLE_OCTY_PUBLIC_KEY=$(base64 < /tmp/octy-local-keys/public.pem)`)
or a Secret/ConfigMap key (Path B) — **or just leave it unset**: every
service falls back to its packaged `keys/octy-public-key.pub` file if the
variable is blank, which won't match your local dev keypair, so any JWT your
local `account` mints will fail verification elsewhere unless you either (a)
override `octy_public_key` with your local public key everywhere, or (b)
overwrite the packaged key file in each service's source tree before
building. (a) is simpler and is what the commands above do.

### ML workers need real AWS to fully exercise

`rfm-worker`, `churn-prediction-worker`, and `recommendation-worker`
orchestrate SageMaker training jobs — that's an opaque external container
image (`*_ALGORITHM_DOCKER_PATH` in their config) with no local emulator.
Their `local-dev/config/*.json` files have placeholder
`AWS_ROLE_ARN`/`*_ALGORITHM_DOCKER_PATH` values so the service starts and
consumes AMQP without crashing on missing config, but the actual
`CreateHyperParameterTuningJob`/`CreateTrainingJob` calls will fail against
those placeholders. To exercise the full training pipeline locally you need
a real AWS account with a SageMaker execution role and the algorithm image
pushed to ECR — there's no way around this, the ML training itself was never
local even in the original Python deployment.

## Troubleshooting

- **A route 500s with "Unexpected error occurred"**: the real error went to
  the component's stderr, not the response body (this is intentional — see
  `rust/README.md`). Check `spin up`'s terminal output (Path A) or `kubectl
  logs` on the SpinApp's pod (Path B).
- **Redis connection refused / TLS errors**: confirm `redis_insecure_tls` is
  set for that service (see §1), and that you're port-forwarding `redis`,
  not connecting to some other Redis on 6379.
- **AMQP messages never arrive at a downstream service**: every gateway
  deployment's `AMQP_EXCHANGE` must be identical (`octy-services` throughout
  `rust/kubernetes/`) — a topic exchange mismatch means messages are
  published into a different routing namespace than consumers are bound to,
  and nothing errors, they just vanish. Check the RabbitMQ management UI
  (`http://localhost:15672` after port-forwarding `rabbitmq` svc port 15672)
  under Exchanges to confirm messages are landing where you expect.
- **`spin build` fails with a missing target**: `rustup target add
  wasm32-wasip1`.
- **MinIO S3 calls fail with a region/constraint error**: some MinIO
  versions are picky about `CreateBucketConfiguration`; this is a known
  rough edge (see `rust/README.md`'s S3 divergences section) — not blocking
  for testing everything except bucket *creation* specifically.

## Clean up

```bash
minikube delete   # or: kubectl delete namespace octy-local
```
