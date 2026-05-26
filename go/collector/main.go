// Package main is the entry point for the Replayable OTel ingest collector.
//
// v0.0.1 stub. Future work per docs/adr/0001 and docs/adr/0002:
// - extend upstream opentelemetry-collector
// - custom receiver normalizing incoming OTLP traces into canonical AgentTrace
// - exporters to ClickHouse (default) and Postgres (small-deploy fallback)
package main

import "fmt"

const version = "0.0.1"

func main() {
	fmt.Printf("replayable-collector %s\n", version)
	fmt.Println("Status: stub — see docs/ARCHITECTURE.md and docs/adr/0001-canonical-trace-schema.md.")
}
