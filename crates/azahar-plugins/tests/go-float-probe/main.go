// Command go-float-probe exposes strconv.ParseFloat acceptance for the numeric
// settings validator. It reads one JSON request per line and writes one JSON
// response per line: {"numbers":[...]} -> {"accepted":[bool,...]}.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"strconv"
)

type request struct {
	Numbers []string `json:"numbers"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 16*1024*1024)
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
		accepted := make([]bool, len(req.Numbers))
		for index, number := range req.Numbers {
			_, err := strconv.ParseFloat(number, 64)
			accepted[index] = err == nil
		}
		writeJSON(writer, map[string]any{"accepted": accepted})
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "go-float-probe: read:", err)
		os.Exit(1)
	}
}

func writeJSON(writer *bufio.Writer, value any) {
	raw, err := json.Marshal(value)
	if err != nil {
		fmt.Fprintln(os.Stderr, "go-float-probe: encode:", err)
		os.Exit(1)
	}
	writer.Write(raw)
	writer.WriteByte('\n')
	if err := writer.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "go-float-probe: flush:", err)
		os.Exit(1)
	}
}
