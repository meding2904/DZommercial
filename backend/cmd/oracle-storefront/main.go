// Differential oracle for the storefront theme package. Reads newline-delimited
// JSON requests and writes one JSON response per line.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/storefront"
)

type request struct {
	Op    string            `json:"op"`
	Raw   string            `json:"raw"`
	Theme *storefront.Theme `json:"theme"`
	FG    string            `json:"fg"`
	BG    string            `json:"bg"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 4*1024*1024)
	writer := bufio.NewWriter(os.Stdout)
	defer writer.Flush()

	for scanner.Scan() {
		line := scanner.Bytes()
		if len(line) == 0 {
			continue
		}
		var req request
		if err := json.Unmarshal(line, &req); err != nil {
			writeJSON(writer, map[string]string{"error": "bad request: " + err.Error()})
			continue
		}
		handle(writer, req)
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-storefront: read:", err)
		os.Exit(1)
	}
}

func handle(writer *bufio.Writer, req request) {
	theme := storefront.Theme{}
	if req.Theme != nil {
		theme = *req.Theme
	}

	switch req.Op {
	case "default":
		writeJSON(writer, storefront.Default())
	case "parse":
		parsed, err := storefront.Parse(req.Raw)
		if err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]any{"theme": parsed})
	case "encode":
		raw, err := storefront.Encode(theme)
		if err != nil {
			writeJSON(writer, map[string]string{"error": err.Error()})
			return
		}
		writeJSON(writer, map[string]string{"raw": raw})
	case "normalize":
		writeJSON(writer, storefront.Normalize(theme))
	case "validate":
		writeJSON(writer, map[string]any{"fields": storefront.Validate(theme)})
	case "contrast":
		writeJSON(writer, map[string]float64{"ratio": storefront.ContrastRatio(req.FG, req.BG)})
	case "parse_lenient":
		writeJSON(writer, storefront.ParseLenient(req.Raw))
	default:
		writeJSON(writer, map[string]string{"error": "unknown op"})
	}
}

func writeJSON(writer *bufio.Writer, value any) {
	raw, err := json.Marshal(value)
	if err != nil {
		raw, err = json.Marshal(map[string]string{"error": err.Error()})
		if err != nil {
			fmt.Fprintln(os.Stderr, "oracle-storefront: encode:", err)
			os.Exit(1)
		}
	}
	writer.Write(raw)
	writer.WriteByte('\n')
	if err := writer.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-storefront: flush:", err)
		os.Exit(1)
	}
}
