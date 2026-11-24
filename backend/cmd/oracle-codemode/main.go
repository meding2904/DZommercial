// Command oracle-codemode is the differential oracle for the Rust code-mode
// port.
//
// One JSON request per line, one JSON response per line. Pure operations cover
// plan parsing, validation, prompts, patches and fingerprints. The `apply` op
// runs the real Go handler against AZAHAR_TEST_DATABASE_URL (default
// http://127.0.0.1:8880) and dumps the state it produced; the Rust test owns
// every `itcm_*` fixture and restores the shared rows it touches.
//
// The pure helpers are unexported, so run this oracle with the build tag:
//
//	go run -tags oracle ./cmd/oracle-codemode
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"fmt"
	"os"
)

const defaultDSN = "http://127.0.0.1:8880"

type fieldInput struct {
	Key      string `json:"key"`
	Label    string `json:"label"`
	Type     string `json:"type"`
	Required bool   `json:"required"`
}

type pluginInput struct {
	ID     string       `json:"id"`
	Label  string       `json:"label"`
	Fields []fieldInput `json:"fields"`
}

type productTypeInput struct {
	Label  string       `json:"label"`
	Fields []fieldInput `json:"fields"`
}

type wireProduct struct {
	ID           string `json:"id"`
	Slug         string `json:"slug"`
	Name         string `json:"name"`
	Subtitle     string `json:"subtitle"`
	Description  string `json:"description"`
	PriceCents   int64  `json:"price_cents"`
	Stock        int    `json:"stock"`
	Active       bool   `json:"active"`
	PluginID     string `json:"plugin_id"`
	CategorySlug string `json:"category_slug"`
}

type wireCategory struct {
	Slug     string `json:"slug"`
	PluginID string `json:"plugin_id"`
}

type wireOrder struct {
	ID     string `json:"id"`
	Number string `json:"number"`
	Status string `json:"status"`
}

type request struct {
	Op string `json:"op"`

	Raw   string          `json:"raw"`
	Plan  string          `json:"plan"`
	Theme json.RawMessage `json:"theme"`
	Patch json.RawMessage `json:"patch"`
	Now   string          `json:"now"`

	Attributes  map[string][]string `json:"attributes"`
	ProductType *productTypeInput   `json:"product_type"`
	Products    []wireProduct       `json:"products"`
	Categories  []wireCategory      `json:"categories"`
	Orders      []wireOrder         `json:"orders"`
	Plugins     []pluginInput       `json:"plugins"`
	Settings    map[string]string   `json:"settings"`
	Fields      map[string]string   `json:"fields"`

	Actor     string `json:"actor"`
	PlanID    string `json:"plan_id"`
	SecretKey string `json:"secret_key"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 8<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var req request
		if err := json.Unmarshal(scanner.Bytes(), &req); err != nil {
			_ = encoder.Encode(map[string]any{"error": err.Error()})
			continue
		}
		_ = encoder.Encode(handle(context.Background(), req))
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
