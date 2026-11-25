//go:build !oracle

// The oracle needs the `oracle` build tag for the plugin-specific exports.
// This stub keeps `go vet ./cmd/oracle-ai` compiling without the tag.
package main

import (
	"fmt"
	"os"
)

func main() {
	fmt.Fprintln(os.Stderr, "oracle-ai: run with `go run -tags oracle ./cmd/oracle-ai`")
	os.Exit(1)
}
