// Command oracle-plugin-data serves the recipes of the two data-only plugins,
// one JSON request per line. Differential tests compare the exact manifest JSON
// with the Rust port.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/plugins"
	"azahar/internal/plugins/oils"
	"azahar/internal/plugins/pcparts"
)

type request struct {
	Plugin string `json:"plugin"`
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
		fmt.Fprintln(os.Stderr, "oracle-plugin-data: read:", err)
		os.Exit(1)
	}
}

func handle(writer *bufio.Writer, req request) {
	switch req.Plugin {
	case "oils":
		writeJSON(writer, map[string]any{"manifests": []plugins.Manifest{oils.New().Manifest()}})
	case "pcparts":
		writeJSON(writer, map[string]any{"manifests": []plugins.Manifest{pcparts.New().Manifest()}})
	case "all":
		writeJSON(writer, map[string]any{"manifests": []plugins.Manifest{
			oils.New().Manifest(),
			pcparts.New().Manifest(),
		}})
	default:
		writeJSON(writer, map[string]string{"error": "unknown plugin"})
	}
}

func writeJSON(writer *bufio.Writer, value any) {
	raw, err := json.Marshal(value)
	if err != nil {
		raw, err = json.Marshal(map[string]string{"error": err.Error()})
		if err != nil {
			fmt.Fprintln(os.Stderr, "oracle-plugin-data: encode:", err)
			os.Exit(1)
		}
	}
	writer.Write(raw)
	writer.WriteByte('\n')
	if err := writer.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-plugin-data: flush:", err)
		os.Exit(1)
	}
}
