// Command oracle-orderstatus-secrets exposes the orderstatus and secrets
// packages over newline-delimited JSON for the Rust differential tests.
package main

import (
	"bufio"
	"encoding/base64"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/orderstatus"
	"azahar/internal/secrets"
)

type request struct {
	Op     string `json:"op"`
	Status string `json:"status"`
	From   string `json:"from"`
	To     string `json:"to"`
	Key    string `json:"key"`
	Plain  string `json:"plain"`
	Value  string `json:"value"`
}

func main() {
	in := bufio.NewScanner(os.Stdin)
	in.Buffer(make([]byte, 64*1024), 8*1024*1024)
	out := bufio.NewWriter(os.Stdout)
	defer out.Flush()

	for in.Scan() {
		var req request
		var resp map[string]any
		if err := json.Unmarshal(in.Bytes(), &req); err != nil {
			resp = map[string]any{"error": "bad request: " + err.Error()}
		} else {
			resp = handle(req)
		}

		payload, err := json.Marshal(resp)
		if err != nil {
			payload = []byte(`{"error":"encode response"}`)
		}
		if _, err := out.Write(payload); err != nil {
			fmt.Fprintln(os.Stderr, "write:", err)
			os.Exit(1)
		}
		if err := out.WriteByte('\n'); err != nil {
			fmt.Fprintln(os.Stderr, "write:", err)
			os.Exit(1)
		}
		if err := out.Flush(); err != nil {
			fmt.Fprintln(os.Stderr, "flush:", err)
			os.Exit(1)
		}
	}
	if err := in.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "read:", err)
		os.Exit(1)
	}
}

func handle(req request) map[string]any {
	switch req.Op {
	case "known":
		return map[string]any{"ok": orderstatus.Known(req.Status)}
	case "can_transition":
		return map[string]any{"ok": orderstatus.CanTransition(req.From, req.To)}
	case "statuses":
		return map[string]any{"statuses": orderstatus.Statuses}
	case "seal":
		plain, err := base64.RawStdEncoding.DecodeString(req.Plain)
		if err != nil {
			return map[string]any{"error": "plain: " + err.Error()}
		}
		value, err := secrets.New(req.Key).Encrypt(plain)
		if err != nil {
			return map[string]any{"error": err.Error()}
		}
		return map[string]any{"value": value}
	case "open":
		plain, err := secrets.New(req.Key).Decrypt(req.Value)
		if err != nil {
			return map[string]any{"error": err.Error()}
		}
		return map[string]any{
			"plain": base64.RawStdEncoding.EncodeToString(plain),
			"error": "",
		}
	default:
		return map[string]any{"error": "unknown op"}
	}
}
