.PHONY: help check check-rust check-go check-python check-ts check-ui test fmt clean

help:
	@echo "Replayable — build targets"
	@echo ""
	@echo "  make check          run all language checks (lint, types, tests)"
	@echo "  make check-rust     cargo fmt --check, clippy, test"
	@echo "  make check-go       gofmt, go vet, go test"
	@echo "  make check-python   ruff, pyright, pytest"
	@echo "  make check-ts       pnpm typecheck, vitest (in ts/)"
	@echo "  make check-ui       pnpm typecheck, vitest (in ui/)"
	@echo "  make fmt            apply formatters across all languages"
	@echo "  make clean          remove build artifacts and caches"

check: check-rust check-go check-python check-ts check-ui

check-rust:
	@command -v cargo >/dev/null || { echo "ERROR: cargo not installed. See https://rustup.rs"; exit 1; }
	cd crates && cargo fmt --check
	cd crates && cargo clippy --all-targets --all-features -- -D warnings
	cd crates && cargo test --workspace

check-go:
	@command -v go >/dev/null || { echo "ERROR: go not installed. Need Go 1.22+"; exit 1; }
	@gofmt_out=$$(cd go && gofmt -l .) ; \
	  if [ -n "$$gofmt_out" ]; then echo "gofmt: needs formatting:"; echo "$$gofmt_out"; exit 1; fi
	cd go && go vet ./...
	cd go && go test ./...

check-python:
	@command -v uv >/dev/null || { echo "ERROR: uv not installed. See https://astral.sh/uv"; exit 1; }
	cd python && uv sync --quiet
	cd python && uv run ruff check .
	cd python && uv run ruff format --check .
	cd python && uv run pyright
	cd python && uv run pytest

check-ts:
	@command -v pnpm >/dev/null || { echo "ERROR: pnpm not installed. Run: npm install -g pnpm"; exit 1; }
	cd ts && pnpm install
	cd ts && pnpm typecheck
	cd ts && pnpm test

check-ui:
	@command -v pnpm >/dev/null || { echo "ERROR: pnpm not installed. Run: npm install -g pnpm"; exit 1; }
	cd ui && pnpm install
	cd ui && pnpm typecheck
	cd ui && pnpm test

test: check

fmt:
	@command -v cargo >/dev/null && cd crates && cargo fmt || echo "skipping cargo fmt (no cargo)"
	@command -v gofmt >/dev/null && cd go && gofmt -w . || echo "skipping gofmt (no gofmt)"
	@command -v uv >/dev/null && cd python && uv run ruff format . || echo "skipping ruff format (no uv)"

clean:
	rm -rf crates/target
	rm -rf python/.venv
	find python -type d \( -name __pycache__ -o -name '.pytest_cache' -o -name '.ruff_cache' -o -name '.pyright_cache' -o -name '*.egg-info' \) -exec rm -rf {} + 2>/dev/null || true
	rm -rf ts/node_modules ts/sdk/node_modules ts/adapters/*/node_modules
	rm -rf ui/node_modules ui/.next
