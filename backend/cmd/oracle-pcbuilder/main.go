// Command oracle-pcbuilder exposes the parts-catalog shaping for Rust
// differential tests.
//
//	go run -tags oracle ./cmd/oracle-pcbuilder
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/plugins/pcbuilder"
	"azahar/internal/store"
)

type request struct {
	Op         string           `json:"op"`
	Categories []store.Category `json:"categories"`
	Products   []store.Product  `json:"products"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 8<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var request request
		if err := json.Unmarshal(scanner.Bytes(), &request); err != nil {
			_ = encoder.Encode(map[string]any{"error": err.Error()})
			continue
		}
		switch request.Op {
		case "parts":
			_ = encoder.Encode(map[string]any{"response": pcbuilder.OracleBuildPartsResponse(request.Categories, request.Products)})
		case "manifest":
			_ = encoder.Encode(map[string]any{"manifest": pcbuilder.New().Manifest()})
		default:
			_ = encoder.Encode(map[string]any{"error": fmt.Sprintf("unknown op %q", request.Op)})
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
