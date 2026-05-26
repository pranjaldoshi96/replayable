package main

import (
	"strings"
	"testing"
)

func TestVersionConstantIsSet(t *testing.T) {
	if version == "" {
		t.Fatal("version constant is empty")
	}
}

func TestVersionLooksLikeSemver(t *testing.T) {
	if !strings.HasPrefix(version, "0.") {
		t.Errorf("version = %q, expected pre-1.0 prefix", version)
	}
}
