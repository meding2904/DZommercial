// Command oracle-ids-auth is a line-oriented differential oracle for the Rust
// ports of azahar/internal/ids and azahar/internal/auth.
//
// It reads one JSON request per line from stdin and writes one JSON response
// per line to stdout. Malformed input produces an error response and does not
// stop the loop.
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/auth"
	"azahar/internal/ids"
)

type request struct {
	Op       string `json:"op"`
	Prefix   string `json:"prefix"`
	Password string `json:"password"`
	Hash     string `json:"hash"`
	Token    string `json:"token"`
}

type response struct {
	ID    string `json:"id,omitempty"`
	Hash  string `json:"hash,omitempty"`
	Token string `json:"token,omitempty"`
	OK    *bool  `json:"ok,omitempty"`
	Error string `json:"error,omitempty"`
}

func handle(req request) (resp response) {
	defer func() {
		if recovered := recover(); recovered != nil {
			resp = response{Error: fmt.Sprintf("panic: %v", recovered)}
		}
	}()

	switch req.Op {
	case "id":
		resp.ID = ids.New(req.Prefix)
	case "hash":
		hash, err := auth.HashPassword(req.Password)
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Hash = hash
	case "verify":
		ok := auth.VerifyPassword(req.Password, req.Hash)
		resp.OK = &ok
	case "new_token":
		token, err := auth.NewToken()
		if err != nil {
			resp.Error = err.Error()
			return resp
		}
		resp.Token = token
	case "hash_token":
		resp.Hash = auth.HashToken(req.Token)
	case "dummy_hash":
		resp.Hash = auth.DummyHash()
	case "verify_dummy":
		auth.VerifyDummy(req.Password)
	default:
		resp.Error = fmt.Sprintf("unknown op %q", req.Op)
	}
	return resp
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 1024*1024)
	writer := bufio.NewWriter(os.Stdout)
	defer writer.Flush()

	for scanner.Scan() {
		line := scanner.Bytes()
		var req request
		if err := json.Unmarshal(line, &req); err != nil {
			resp := response{Error: "invalid JSON: " + err.Error()}
			encoded, _ := json.Marshal(resp)
			if _, writeErr := writer.Write(append(encoded, '\n')); writeErr != nil {
				return
			}
			if flushErr := writer.Flush(); flushErr != nil {
				return
			}
			continue
		}

		encoded, err := json.Marshal(handle(req))
		if err != nil {
			encoded, _ = json.Marshal(response{Error: "encode response: " + err.Error()})
		}
		if _, err := writer.Write(append(encoded, '\n')); err != nil {
			return
		}
		if err := writer.Flush(); err != nil {
			return
		}
	}

	if err := scanner.Err(); err != nil {
		encoded, _ := json.Marshal(response{Error: "read stdin: " + err.Error()})
		_, _ = writer.Write(append(encoded, '\n'))
		_ = writer.Flush()
	}
}
