// Package main is the entry point for agentctl, the Replayable CLI.
//
// v0.0.1 stub. Future work per docs/ARCHITECTURE.md:
// - subcommands: capture, replay, eval, trace, deploy
// - cobra-based CLI structure
// - distributed as a single static binary
package main

import "fmt"

const version = "0.0.1"

func main() {
	fmt.Printf("agentctl %s\n", version)
	fmt.Println("Status: stub — see docs/ARCHITECTURE.md for component layout.")
}
