//go:build !oracle

package main

import (
	"fmt"

	"azahar/internal/plugins"
)

// optionsResult is unavailable without the oracle build tag; the differential
// test runs `go run -tags oracle`.
func optionsResult(_ *plugins.Registry, _ request) map[string]any {
	return errorResult(fmt.Errorf("the options op needs `go run -tags oracle`"))
}
