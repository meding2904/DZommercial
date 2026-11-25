// Command oracle-validate exposes the unexported validation helpers through a
// line-delimited JSON protocol for the Rust differential tests.
//
//	go run -tags oracle ./cmd/oracle-validate
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/httpapi"
)

type request struct {
	Op     string   `json:"op"`
	Value  string   `json:"value"`
	Values []string `json:"values"`
	Max    int      `json:"max"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 4<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var request request
		if err := json.Unmarshal(scanner.Bytes(), &request); err != nil {
			_ = encoder.Encode(map[string]any{"error": err.Error()})
			continue
		}
		response := map[string]any{}
		switch request.Op {
		case "email":
			response["ok"] = httpapi.OracleValidEmail(request.Value)
		case "slug":
			response["ok"] = httpapi.OracleValidSlug(request.Value)
		case "phone":
			response["ok"] = httpapi.OracleValidPhone(request.Value)
		case "mobile":
			value, ok := httpapi.OracleNormalizedAlgerianMobile(request.Value)
			response["value"] = value
			response["ok"] = ok
		case "rune_len":
			response["value"] = httpapi.OracleRuneLen(request.Value)
		case "truncate":
			response["value"] = httpapi.OracleTruncateRunes(request.Value, request.Max)
		case "trim_all":
			response["values"] = httpapi.OracleTrimAll(request.Values)
		default:
			response["error"] = fmt.Sprintf("unknown op %q", request.Op)
		}
		if err := encoder.Encode(response); err != nil {
			fmt.Fprintln(os.Stderr, err)
			os.Exit(1)
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
