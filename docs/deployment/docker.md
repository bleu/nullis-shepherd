# Docker deployment runbook

Operator-facing quickstart for running Shepherd in production via the
published container image. For the full hardening surface (systemd
unit, backup recipes, RPC selection, alerting rules) read
`docs/production.md`.

The image is published on every push to `main` and on every
`v*` tag:

```
ghcr.io/bleu/nullis-shepherd:latest         # main branch HEAD
ghcr.io/bleu/nullis-shepherd:sha-<short>    # exact-build pin
ghcr.io/bleu/nullis-shepherd:v0.2.0         # tag
```

`linux/amd64` only for now (the soak VM is x86_64; add `arm64` once
an operator surfaces a real need).

---

## 1. First boot on a fresh VM

```bash
# On the VM:
git clone https://github.com/bleu/nullis-shepherd /opt/shepherd
cd /opt/shepherd

# Operator-supplied config. Start from the example, fill in the
# paid-RPC URL (Alchemy / Infura / QuickNode) for every chain you
# want the engine to subscribe to.
cp engine.example.toml engine.toml
${EDITOR:-vi} engine.toml

# Pull the published image (no local build needed).
docker compose pull

# Start the engine.
docker compose up -d

# Logs (JSON line-per-event, see `docs/production.md §5`).
docker compose logs -f shepherd
```

If you want the observability stack on the same host:

```bash
docker compose --profile observability up -d
# Prometheus UI: http://127.0.0.1:9090
```

The metrics endpoint binds the **host's loopback** by default
(`127.0.0.1:9100`); the Prometheus container scrapes via the
compose-internal DNS name `shepherd:9100`. Never expose `:9100` to
the public internet without authn — see `docs/production.md §7`.

---

## 2. Configuring `engine.toml`

The image bind-mounts `./engine.toml` at `/etc/shepherd/engine.toml`
read-only. Minimum production shape:

```toml
[engine]
state_dir = "/var/lib/shepherd"   # mapped to the `shepherd-state` named volume
log_level = "info"

[engine.metrics]
enabled = true
bind_addr = "0.0.0.0:9100"        # inside the container; compose maps to 127.0.0.1

# One per chain you subscribe to. WS URLs unlock `eth_subscribe`
# (block + log streams); HTTP URLs degrade to polling and are not
# recommended for production.
[chains.11155111]
rpc_url = "wss://eth-sepolia.g.alchemy.com/v2/<KEY>"

[chains.42161]
rpc_url = "wss://arb-mainnet.g.alchemy.com/v2/<KEY>"

# One [[modules]] per .wasm baked into /opt/shepherd/modules/.
# `manifest` defaults to <path-parent>/module.toml if omitted.
[[modules]]
path = "/opt/shepherd/modules/twap_monitor.wasm"
manifest = "/opt/shepherd/manifests/twap-monitor.toml"

[[modules]]
path = "/opt/shepherd/modules/ethflow_watcher.wasm"
manifest = "/opt/shepherd/manifests/ethflow-watcher.toml"
# Add price-alert / balance-tracker / stop-loss the same way.
```

For convenience, `engine.docker.toml` in the repo root ships the
exact module path layout the image bakes; copy it as `engine.toml`,
swap the placeholder RPC URLs, and you're done:

```bash
cp engine.docker.toml engine.toml
${EDITOR:-vi} engine.toml   # replace <RPC_KEY> placeholders
```

Public RPCs throttle `eth_subscribe` + `eth_getLogs` under sustained
load (independently confirmed by the baseline-latency tool — see
`docs/operations/baselines/`). The soak (COW-1031) explicitly
requires paid endpoints.

---

## 3. Upgrade / rollback

```bash
# Roll forward to the latest main-branch build.
docker compose pull
docker compose up -d              # picks up the new image; graceful
                                  # shutdown drains in-flight dispatch
                                  # (COW-1072) before the new container
                                  # takes over.

# Roll back to a specific build.
export SHEPHERD_IMAGE=ghcr.io/bleu/nullis-shepherd:sha-abc1234
docker compose up -d

# Cold roll: stop, prune image, pull fresh.
docker compose down
docker image rm ghcr.io/bleu/nullis-shepherd:latest
docker compose pull && docker compose up -d
```

The `shepherd-state` named volume survives container recreation —
the redb file with all `submitted:` / `dropped:` / `backoff:` markers
persists across upgrades by design (idempotency lives there).

---

## 4. Building the image locally

The CI publishes on every push, so the local build path is only for
testing un-merged changes:

```bash
docker compose build               # uses repo-root Dockerfile
docker compose up -d               # runs the locally-built image
```

To pin the locally-built tag and avoid accidentally pulling `:latest`:

```bash
export SHEPHERD_IMAGE=shepherd:local
docker build -t "$SHEPHERD_IMAGE" .
docker compose up -d
```

---

## 5. Verifying the deploy

```bash
# Engine is up, modules are loaded, no module is quarantined.
curl -s http://127.0.0.1:9100/metrics \
    | grep -E '^shepherd_(module_poisoned|module_restarts_total|stream_reconnects_total)'

# Tail the structured logs.
docker compose logs -f shepherd | grep -E '"level":(("ERROR")|("WARN"))'

# In a separate shell: confirm the engine wrote a last-dispatched-
# block marker after the first 30s of uptime (proof the supervisor
# is dispatching events, not just idle-looping).
docker compose exec shepherd ls -la /var/lib/shepherd/
```

Green: `shepherd_module_poisoned == 0`, no ERROR/WARN lines beyond
boot, and a non-empty redb file under `/var/lib/shepherd/`.

---

## 6. Cross-references

- `docs/production.md` — full process-level deploy (systemd path),
  backup recipes, RPC selection, alerting rules, runbook.
- `docs/06-production-hardening.md` — resource-limit design (fuel,
  memory, storage), restart policy, RPC resilience, observability
  design.
- `docs/operations/m3-testnet-runbook.md` — staging validation
  playbook; reuse the same steps before the production soak.
- `engine.example.toml` — annotated reference for the engine config.
