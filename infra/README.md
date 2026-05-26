# infra

Local development infrastructure.

## Bring up storage services

```bash
docker compose -f infra/docker-compose.yml up -d
```

- ClickHouse: `http://localhost:8123` (HTTP), `localhost:9000` (native TCP)
- Postgres:   `localhost:5432`

Default credentials are `replayable / replayable / replayable`.
**Replace these before any non-local use.**

## Tear down

```bash
docker compose -f infra/docker-compose.yml down
```

Add `-v` to also remove named volumes (`clickhouse_data`, `postgres_data`)
and start fresh next time.

## Roadmap

- `Dockerfile.server` — image for the Python API server (uncomment the
  `server` service block once it lands).
- Helm chart for Kubernetes deployments (v2 scope per docs/PRD.md).
