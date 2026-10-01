# CKB MCP / Reality on the shared VPS

This profile runs the public Rust CKB MCP/Reality service under `/opt/platform/ckb/core` without taking ownership of the public 80/443 edge or interfering with other co-hosted services.

## Host contract

- The shared platform gateway owns 80/443.
- MCP exposes only localhost diagnostic port `8320` and the private external `ckb_mesh` network.
- CKB API reaches MCP as `http://ckb-mcp:3000`.
- MCP validates per-user API keys through CKB API as `http://ckb-api:3000` using the shared CKB-only internal secret.
- `CKB_BIND_ALL=1` is required inside Docker. Without it the Rust server binds `127.0.0.1` inside the container and other containers cannot reach it.

## First rollout

Create the private network once if it does not already exist:

```bash
docker network inspect ckb_mesh >/dev/null 2>&1 || docker network create ckb_mesh
```

From `/opt/platform/ckb/core`:

```bash
cp deploy/shared-vps/.env.example deploy/shared-vps/.env
chmod 600 deploy/shared-vps/.env
# Set CKB_INTERNAL_SECRET to exactly the same value used by ckb-cloud.

docker compose --env-file deploy/shared-vps/.env \
  -f deploy/shared-vps/docker-compose.yml config >/tmp/ckb-mcp-compose.rendered.yml

docker compose --env-file deploy/shared-vps/.env \
  -f deploy/shared-vps/docker-compose.yml build ckb-mcp

docker compose --env-file deploy/shared-vps/.env \
  -f deploy/shared-vps/docker-compose.yml up -d ckb-mcp
```

Verify locally:

```bash
curl --fail http://127.0.0.1:8320/health
```

Then verify the API container can resolve/reach `ckb-mcp:3000` across `ckb_mesh` before any frontend/backend cutover.

## Rollback

```bash
docker compose --env-file deploy/shared-vps/.env \
  -f deploy/shared-vps/docker-compose.yml stop ckb-mcp
```

Rollback is CKB-local. Do not restart or reconfigure unrelated co-hosted services during a CKB rollback.
