// Command oracle-db exposes the migration SQL splitter for Rust differential
// tests.
//
//	go run -tags oracle ./cmd/oracle-db
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/db"
)

type request struct {
	Body string `json:"body"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 16<<20)
	encoder := json.NewEncoder(os.Stdout)
	for scanner.Scan() {
		var request request
		if err := json.Unmarshal(scanner.Bytes(), &request); err != nil {
			_ = encoder.Encode(map[string]any{"error": err.Error()})
			continue
		}
		_ = encoder.Encode(map[string]any{"statements": db.OracleSplitStatements(request.Body)})
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
