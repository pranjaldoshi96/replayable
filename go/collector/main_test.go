package main

import "testing"

func TestCollectorVersionIsSet(t *testing.T) {
	if version == "" {
		t.Fatal("version constant is empty")
	}
}
