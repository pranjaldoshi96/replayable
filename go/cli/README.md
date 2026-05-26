# agentctl

The Replayable command-line interface.
A single static binary that drives every non-visual workflow: capture sidecar lifecycle, trace inspection, dataset CRUD, replay, eval runs, and judge calibration.

## Status

**v0.0.1 — stub.**
The binary builds and prints a version banner.
Subcommands are not implemented yet.

## Build and run

```bash
# from the repo root
cd go/cli
go build -o agentctl .
./agentctl

# or, without building:
go run .
```

Tests and vet:

```bash
go test ./...
go vet ./...
gofmt -l .
```

`make check-go` runs the above from the repo root across the whole Go workspace.

## Planned surface

```text
agentctl trace list
agentctl trace get <trace-id>
agentctl dataset add --trace <trace-id> --dataset <name>
agentctl replay <trace-id> [--live-tool <name>] [--prompt-override @file]
agentctl eval run --dataset <name> --evaluators ...
agentctl capture start | stop
```

CLI parity with the web UI is required for every non-visual operation (PRD FR-UI-03).

## References

- [ARCHITECTURE.md §2](../../docs/ARCHITECTURE.md) — `agentctl` container row, deployment slot.
- [ARCHITECTURE.md §6](../../docs/ARCHITECTURE.md) — single-binary distribution.
- PRD FR-UI-03 / DEP-03 — CLI verbs and single-binary requirement.

## Roadmap (v0.1.0)

- `cobra`-based command tree with `--help` listing every v1 verb.
- REST client against the API server (`/healthz`, `/traces`, `/datasets`).
- OTLP client for capture-sidecar control.
- `pipx`-installable Python wrapper for Tier-1 ergonomics (PRD DEP-03).
- Release pipeline producing static binaries for Linux, macOS, and Windows.
