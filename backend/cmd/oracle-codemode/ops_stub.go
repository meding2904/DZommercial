//go:build !oracle

package main

import "context"

// handle is unavailable without the oracle build tag; the differential test
// runs `go run -tags oracle`.
func handle(_ context.Context, _ request) map[string]any {
	return map[string]any{"error": "the code-mode oracle needs `go run -tags oracle`"}
}
