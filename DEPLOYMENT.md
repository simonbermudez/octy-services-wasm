# Octy legacy deployment guide (Python)

This documents the **original Python microservices** at the repo root (12
FastAPI services + 5 background workers). If you're looking for the Rust/WASM
rewrite instead, see [`rust/README.md`](rust/README.md) and
[`rust/local-dev/README.md`](rust/local-dev/README.md) — the two stacks share
no runtime dependency, but this guide reuses the Rust rewrite's local
infrastructure manifests and per-service config files where they're
identical (they were derived from this exact codebase's config schema).

## Architecture at a glance

| Service | Directory | Purpose | Service port | Public route | Ingress middleware |
|---|---|---|---|---|---|
| account | `account/` | Account CRUD, pk/sk authentication, JWT minting | 1024 | `/v1/admin/account/create`, `/v1/account/authenticate` | `admin-auth` (create only) |
| admin | `admin/` | App version tracking (GitHub release webhook), bundled resource-format docs | 1025 | `/v1/admin/application/*` | **none** — see [known gaps](#known-gaps--recommendations) |
| billing | `billing/` | Usage-based billing unit tracking, subscription plans | 1040 | `/v1/admin/billing/*` | `admin-auth` |
| configurations | `configurations/` | Per-account/algorithm configuration | 1026 | `/v1/configurations/*` | `forward-auth` |
| events | `events/` | Event + event-type ingestion | 1029 | `/v1/retention/events*` | `forward-auth` |
| items | `items/` | Item catalog CRUD | 1028 | `/v1/retention/items*` | `forward-auth` |
| messaging | `messaging/` | Message templates, Rybbon reward-card integration | 1033 | `/v1/retention/messaging*` | `forward-auth` |
| octy_jobs | `octy_jobs/` | Scheduled/recurring job orchestration (drives the 5 workers below) | 1034 | — (internal only) | — |
| profiles | `profiles/` | Customer profile CRUD, segment tags | 1027 | `/v1/retention/profiles*` | `forward-auth` |
| recommendation | `recommendation/` | Recommendation cache lookups | 1032 | `/v1/retention/recommendations*` | `forward-auth` |
| segmentation | `segmentation/` | Segment CRUD | 1030 | `/v1/retention/segments*` | `forward-auth` |
| churn_prediction | `churn_prediction/` | Churn report retrieval | 1031 | `/v1/retention/churn_prediction*` | `forward-auth` |
| workers/rfm | `workers/rfm/` | RFM analysis (SageMaker training + scoring) | — | — (AMQP-triggered) | — |
| workers/churn_prediction | `workers/churn_prediction/` | Churn model training (SageMaker + XGBoost) | — | — (AMQP-triggered) | — |
| workers/recommendation | `workers/recommendation/` | Recommender training (SageMaker + LightFM) | — | — (AMQP-triggered) | — |
| workers/segmentation | `workers/segmentation/` | Past/live segment evaluation | — | — (AMQP-triggered) | — |
| workers/profile_identification | `workers/profile_identification/` | Cross-device profile identity resolution | — | — (AMQP-triggered) | — |

All 17 are independent FastAPI apps (the 5 workers expose only `/healthz` —
their real work runs in an AMQP-consumer background thread started at
`@app.on_event('startup')`). Shared infrastructure: MongoDB (`motor`/
`mongoengine`), Redis (TLS-only, see [below](#redis-requires-tls-unconditionally)),
RabbitMQ (via the [`octy-rabbitmq-lib`](https://github.com/Octy-ai/octy-rabbitmq-lib)
package, now public — see [prerequisites](#0-prerequisites-read-this-first)),
AWS S3, AWS SageMaker (the 3 ML workers), Mailjet (transactional email), and
Sentry.

### How authentication actually works

Individual services mostly just decode a JWT (`X-AUTH-JWT` header,
`decode_account_jwt` in each service's `api/routers/utils.py`) — **they never
see a raw public/secret key pair**. That's because
[`kubernetes/production/traefik-ingress/traefik-ingress-routes.yaml`](kubernetes/production/traefik-ingress/traefik-ingress-routes.yaml)
attaches a `forward-auth` Traefik middleware to every `/v1/retention/*` and
`/v1/configurations/*` route, which calls
`https://api.octy.ai/v1/account/authenticate` (i.e. the account service, with
the client's original Basic-Auth header) on every request and forwards the
`X-AUTH-JWT` response header downstream. The account service is the only one
that ever verifies a pk/sk pair directly. Two other middlewares exist:
`admin-auth` (Traefik `basicAuth`, gates account creation + billing admin
routes) and `trusted-app-auth` (defined in
[`kubernetes/production/middlewares/middlewares.yaml`](kubernetes/production/middlewares/middlewares.yaml)
but **not currently attached to any route** — see below).

## 0. Prerequisites (read this first)

- **Python 3.11** for the 12 API services (`FROM python:3.11-slim` in their
  Dockerfiles); the 5 workers' Dockerfiles use the older
  `tiangolo/uvicorn-gunicorn-fastapi:python3.7` base image instead — a
  pre-existing inconsistency, not something introduced by this guide. Either
  version runs the actual application code fine locally; match whichever the
  service's own `Dockerfile` uses if you're building images.
- Every service imports `octy_rabbitmq.amqp_publisher`/`amqp_consumer` from
  [`Octy-ai/octy-rabbitmq-lib`](https://github.com/Octy-ai/octy-rabbitmq-lib) —
  **this package is public now**, so no GitHub token or repo access is
  needed. `pip install -r requirements.txt` alone still won't pull it in
  (it's not in any `requirements.txt`, it's installed as a separate step in
  every Dockerfile and in CI), so install it explicitly before each
  service's own `requirements.txt`:
  ```bash
  pip install git+https://github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0
  ```
  (Previously this required a GitHub token with read access to the repo —
  that's no longer the case. The Dockerfiles and `scripts/ci-deploy.sh`
  still pass a `git_token` build arg into the equivalent `pip install`
  command; it's harmless to keep supplying one, but an empty/omitted value
  now works too since the repo is public.)
- **Docker** and, for the Kubernetes path, **minikube** + **kubectl**.
- ⚠️ **Before doing anything else**: [`.circleci/config.yml:15`](.circleci/config.yml)
  has a commented-out line containing what looks like a live GitHub personal
  access token. If it's still valid, revoke it in GitHub's token settings —
  a `git log` history rewrite doesn't help once a token has been exposed,
  only revocation does.

## 1. Local development

### Fast path: one service, no Kubernetes

```bash
cd account   # or any other service directory
python3.11 -m venv venv && source venv/bin/activate
pip install git+https://github.com/Octy-ai/octy-rabbitmq-lib.git@1.2.0
pip install -r requirements.txt
```

Every service reads its whole configuration from two base64-encoded-JSON
environment variables — `{SERVICE}_CONFIG` / `{SERVICE}_SECRETS` (e.g.
`ACCOUNT_CONFIG`/`ACCOUNT_SECRETS`; see each service's `config.py`/
`secrets.py`, and `app_secrets.py` where that's the actual filename). This is
the **exact same contract** the Rust rewrite kept, so the same local-dev
config files work for both stacks:

```bash
export ACCOUNT_CONFIG=$(base64 -i ../rust/local-dev/config/account.config.json)
export ACCOUNT_SECRETS=$(base64 -i ../rust/local-dev/config/account.secrets.json)
export OCTY_PRIVATE_KEY=$(base64 -i /tmp/octy-local-keys/private.pem)   # account only, see below
```

(Generate `/tmp/octy-local-keys/{private,public}.pem` per
[`rust/local-dev/README.md`](rust/local-dev/README.md#2-generate-a-local-jwt-keypair) —
it's the same RS256 keypair contract on both stacks.)

Those config files point at `mongo.octy-local`, `redis.octy-local`, and
`rabbitmq.octy-local` — in-cluster hostnames. To resolve them from a bare
`venv` (no Kubernetes), either edit your copies to use `localhost` and
port-forward the backends (see the next section), or run the whole stack in
minikube as described below.

Run it:

```bash
uvicorn api:app --host 0.0.0.0 --port 8080 --reload --log-config api/log-conf.yml
# workers use a different app module name:
#   uvicorn worker:app --host 0.0.0.0 --port 8080 --reload --log-config worker/log-conf.yml
curl http://localhost:8080/healthz
```

### Local infrastructure (MongoDB / Redis / RabbitMQ / S3)

Reuse the Rust rewrite's infra manifests — they're plain Kubernetes
manifests with nothing Rust-specific in them:

```bash
minikube start --cpus=4 --memory=8192
kubectl apply -f rust/local-dev/infra/namespace.yaml
kubectl apply -f rust/local-dev/infra/mongo.yaml
kubectl apply -f rust/local-dev/infra/rabbitmq.yaml
kubectl apply -f rust/local-dev/infra/redis.yaml
kubectl apply -f rust/local-dev/infra/minio.yaml   # optional, for S3 code paths

kubectl -n octy-local port-forward svc/mongo 27017:27017 &
kubectl -n octy-local port-forward svc/redis 6379:6379 &
kubectl -n octy-local port-forward svc/rabbitmq 5672:5672 &
```

#### Redis requires TLS unconditionally

Every service's `data/context/db_context.py` connects with
`ssl=True, ssl_ca_certs=certifi.where()` — hardcoded, with no config
override. A plain local Redis will fail the handshake immediately. This is
the identical constraint the Rust rewrite hit (see
`rust/crates/octy-spin/src/ctx.rs`'s `redis_address()` comment for the full
explanation), and `rust/local-dev/infra/redis.yaml` already solves it by
generating a self-signed cert on pod start. The Python client needs the
matching escape hatch — `redis.asyncio.Redis(..., ssl=True,
ssl_cert_reqs=None)` skips certificate verification while keeping the
connection encrypted (the `redis-py` equivalent of `redis-rs`'s `#insecure`
URL suffix). **This requires a one-line source change** (`ssl_cert_reqs=None`
in `db_context.py`) to work against the local self-signed cert — the Rust
rewrite made this change behind an opt-in, production-inert flag; doing the
same here (rather than editing `certifi.where()`'s CA bundle) is the
lowest-risk path if you want Redis working locally. This file wasn't changed
as part of this documentation pass — see [known gaps](#known-gaps--recommendations).

### Local Kubernetes deploy (mirrors production)

```bash
docker build -t octy-account-service:dev -f account/Dockerfile .   # git_token build arg no longer needed, octy-rabbitmq-lib is public
minikube image load octy-account-service:dev
# repeat per service — `minikube image load` copies a locally-built image
# straight into the minikube node, so no registry/push step is needed at all
# for plain container images (unlike the Rust rewrite's Spin OCI artifacts,
# which need a real OCI registry — see rust/local-dev/README.md Path B).
```

Then apply that service's own `kubernetes/development/` manifests — but
read [known gaps](#known-gaps--recommendations) first, several of them don't
work as committed.

## 2. Production deployment

This section is the original deployment runbook
(`Octy_K8_Deployment_Doc.md`), corrected and filled in against what's
actually in this repository.

### Namespace

```bash
kubectl create ns octy-production
```

### Traefik

```bash
helm repo add traefik https://helm.traefik.io/traefik
helm repo update
helm install traefik traefik/traefik --namespace traefik --create-namespace
kubectl get pods -n traefik
kubectl port-forward {pod name} 9000:9000 -n traefik
# http://127.0.0.1:9000/dashboard/#/ should show Traefik running
```

### cert-manager

```bash
helm repo add jetstack https://charts.jetstack.io
helm repo update
helm install cert-manager jetstack/cert-manager \
  --namespace cert-manager --create-namespace \
  --version v1.10.0 --set installCRDs=true
```

### DNS + SSL

```bash
kubectl get services -n traefik
# repoint your domain's A record at the traefik LoadBalancer's EXTERNAL-IP —
# propagation can take a few minutes
kubectl apply -f kubernetes/production/cert-manager/production/production-cluster-issuer.yaml
kubectl apply -f kubernetes/production/cert-manager/production/production-cert.yaml
```

### Docker Hub pull secret

```bash
kubectl create secret docker-registry docker-hub-key \
  --docker-server=https://index.docker.io/v1/ \
  --docker-username=<dockerhub-username> \
  --docker-password=<dockerhub-password> \
  --docker-email=<email> \
  -n octy-production
```

(The original doc had a real Docker Hub password inline here — rotate it if
it hasn't been already, and never put credentials directly in a runbook;
pull them from a secrets manager or prompt for them interactively.)

### Auth secrets (not checked into this repo — create them yourself)

The ingress middlewares reference three Kubernetes Secrets that don't exist
anywhere in this repository (correctly — they hold credentials and must
never be committed). Create them once per cluster:

```bash
# used by the account service to sign JWTs — RS256 private key PEM
kubectl create secret generic octy-private-key \
  --from-literal=octy_private_key="$(base64 -i private.pem)" -n octy-production

# HTTP Basic Auth for the admin-auth middleware (account creation, billing admin)
htpasswd -nb <admin-user> <admin-password> | kubectl create secret generic \
  admin-authsecret --from-file=users=/dev/stdin -n octy-production

# HTTP Basic Auth for the trusted-app-auth middleware (currently unused — see below)
htpasswd -nb <app-user> <app-password> | kubectl create secret generic \
  trusted-app-authsecret --from-file=users=/dev/stdin -n octy-production
```

### Ingress

```bash
kubectl apply -f kubernetes/production/middlewares/middlewares.yaml
kubectl apply -f kubernetes/production/traefik-ingress/traefik-ingress-routes.yaml
```

### Per-service deploy

For each service (`deployments.json` at the repo root has the authoritative
image name / Dockerfile / manifest path for all 17 — this is what CI actually
reads, and is more trustworthy than any prose description):

```bash
# 1. build + push the image (matches scripts/ci-deploy.sh; that script still
#    passes a git_token build arg, which is now optional since
#    octy-rabbitmq-lib is public)
docker build -t <dockerhub-user>/octy-account-service:latest -f account/Dockerfile .
docker push <dockerhub-user>/octy-account-service:latest

# 2. base64-encode config/secrets into a ConfigMap/Secret
#    (rust/local-dev/encode.sh works unchanged — same base64(JSON) contract).
#    account/config.json and account/secrets.json here are real,
#    production-specific files you maintain locally and keep out of git
#    (the root .gitignore already excludes *.json) — not the placeholder
#    values in rust/local-dev/config/, which are for local dev only.
./rust/local-dev/encode.sh -f account/config.json -y /tmp/account-configmap.yaml \
  -k account_config -n account-config -N octy-production
kubectl apply -f /tmp/account-configmap.yaml
./rust/local-dev/encode.sh -f account/secrets.json -y /tmp/account-secrets.yaml \
  -k account_secrets -n account-secrets -N octy-production -s
kubectl apply -f /tmp/account-secrets.yaml

# 3. deployment + service
kubectl apply -f account/kubernetes/production/deployments/account-deployment.yaml
kubectl apply -f account/kubernetes/production/services/account-service.yaml
```

Repeat for all 17 services/workers listed in `scripts/deployments.json`.

### Wire up internal cluster IPs

Several services call each other directly (account's deletion fan-out,
messaging → items/recommendation, segmentation → events, the ML workers →
event/profile/item/octy-jobs services, etc.). Once every Service is applied:

```bash
kubectl get services -n octy-production
```

and fill in each service's `config.json` with the resulting ClusterIPs
before the `encode.sh` step above, e.g.:

```json
"EVENT_SERVICE_CLUSTER_IP": "http://10.x.x.x:1029",
"PROFILE_SERVICE_CLUSTER_IP": "http://10.x.x.x:1027",
"ITEM_SERVICE_CLUSTER_IP": "http://10.x.x.x:1028",
"SEGMENTATION_SERVICE_CLUSTER_IP": "http://10.x.x.x:1030",
"OCTY_JOB_SERVICE_CLUSTER_IP": "http://10.x.x.x:1034",
"REC_SERVICE_CLUSTER_IP": "http://10.x.x.x:1032"
```

This is manual and fragile by design of the existing config format — a raw
ClusterIP is not stable across a Service recreation, only across pod
restarts. A Kubernetes Service DNS name (`http://events-service.octy-production.svc.cluster.local:1029`,
or even just `http://events-service:1029` from within the namespace) would
survive a Service recreation and remove this step entirely; that's how the
Rust rewrite's manifests are wired (see `rust/kubernetes/*/`). Changing the
17 services' config schemas to match wasn't done as part of this pass since
it touches values baked into already-deployed config blobs, but it's a
straightforward, low-risk follow-up if you want to stop doing this by hand
on every deploy.

### Horizontal Pod Autoscaling

```bash
kubectl autoscale deploy/account-deploy --cpu-percent=95 --min=1 --max=10 -n octy-production
# ...repeat per deployment; see the original doc for the full list.
```

### Rolling restart (after a config change, without a new image)

```bash
kubectl -n octy-production rollout restart deployment account-deploy
# ...repeat per deployment.
```

## Known gaps & recommendations

Found while reading every service's source and Kubernetes manifests to write
this guide — worth triaging regardless of whether you act on this doc's
Rust-rewrite sibling:

- **Admin service routes are unprotected in production.** `/v1/admin/application/versioning`,
  `/versioning/hook`, and `/resources/format` have **no middleware** in
  `kubernetes/production/traefik-ingress/traefik-ingress-routes.yaml`, despite
  the Python route docstrings explicitly stating `Requires auth: YES
  (Trusted App auth)`. The `trusted-app-auth` middleware already exists in
  `middlewares.yaml` and is defined but attached to nothing — it looks like
  it was meant for exactly these routes and the ingress rule was never added.
  The webhook route (`/versioning/hook`) verifies its own HMAC signature
  in-code so it's not fully open, but the other two are genuinely public
  right now. Recommend attaching `trusted-app-auth` to those two routes.
- **Several `development` deployment manifests don't actually work as
  committed.** `messaging` and `octy_jobs`'s dev manifests
  (`*/kubernetes/development/deployments/*.yaml`) are stale templates from an
  older FluxCD-based setup: they reference a placeholder image
  (`aimvector/python:1.0.4`), have no `namespace`, and mount config/secrets as
  **file volumes** at `/configs//secrets/` — a mechanism the application code
  doesn't read from at all (it reads `{SERVICE}_CONFIG`/`{SERVICE}_SECRETS`
  env vars, same as production). Six more services' dev manifests (account,
  configurations, recommendation, churn_prediction, profiles, segmentation,
  events, items) have the `env:` block containing those same env-var bindings
  **commented out** entirely, so the config maps/secrets referenced elsewhere
  in the same manifest are created but never actually attached to the pod.
  `admin`'s dev manifest wires `ADMIN_CONFIG` but not `ADMIN_SECRETS`. The
  4 worker dev manifests (churn_prediction, recommendation, rfm, segmentation)
  and every **production** manifest are correctly wired — use the production
  shape as your reference if you need to fix a dev manifest, or use this
  guide's [per-service deploy](#per-service-deploy) section, which shows the
  correct pattern directly.
- ~~`octy-rabbitmq-lib` gated all local development behind private-repo
  access~~ — **resolved**, the repo is now public (see
  [prerequisites](#0-prerequisites-read-this-first)); no GitHub token needed
  to install it.
- **A likely live GitHub token is committed** in `.circleci/config.yml` — see
  [prerequisites](#0-prerequisites-read-this-first).
- **Redis requires TLS with no local-dev override** — see
  [Redis requires TLS unconditionally](#redis-requires-tls-unconditionally).
- **AMQP exchange name consistency is worth auditing.** The Rust rewrite of
  this same system had 6 of 17 services' gateway configs disagreeing on the
  RabbitMQ exchange name (`"octy"` vs `"octy-services"`), which would
  silently drop every cross-service message between mismatched pairs with no
  error anywhere (topic exchanges are independent namespaces). That
  divergence was introduced during the port, not copied from here, but it's
  exactly the kind of mistake that's easy to make and invisible at runtime —
  worth a quick check that every service's real `Config['EXCHANGE']` value
  agrees, since nothing will complain if they don't.
