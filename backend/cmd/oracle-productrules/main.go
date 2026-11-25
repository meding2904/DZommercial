// Command oracle-productrules exposes the product write rules for Rust
// differential tests.
//
//	go run ./cmd/oracle-productrules
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/plugins"
	"azahar/internal/productrules"
)

type optionField struct {
	Key      string `json:"key"`
	Label    string `json:"label"`
	Type     string `json:"type"`
	Required bool   `json:"required"`
}

type request struct {
	Op         string              `json:"op"`
	Value      string              `json:"value"`
	Cents      int64               `json:"cents"`
	Stock      int                 `json:"stock"`
	Fields     []optionField       `json:"fields"`
	Attributes map[string][]string `json:"attributes"`
}

func (r request) productType() *plugins.ProductType {
	fields := make([]plugins.OptionField, 0, len(r.Fields))
	for _, field := range r.Fields {
		fields = append(fields, plugins.OptionField{
			Key: field.Key, Label: field.Label, Type: field.Type, Required: field.Required,
		})
	}
	return &plugins.ProductType{Label: "oracle", Fields: fields}
}

type problem struct {
	Field string `json:"field"`
	Kind  int    `json:"kind"`
	Label string `json:"label"`
	Limit int    `json:"limit"`
}

func outProblems(problems []productrules.Problem) []problem {
	out := make([]problem, 0, len(problems))
	for _, item := range problems {
		out = append(out, problem{
			Field: item.Field, Kind: int(item.Kind), Label: item.Label, Limit: item.Limit,
		})
	}
	return out
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
		response := map[string]any{}
		switch request.Op {
		case "name":
			response["ok"] = productrules.ValidName(request.Value)
		case "price":
			response["ok"] = productrules.ValidPrice(request.Cents)
		case "stock":
			response["ok"] = productrules.ValidStock(request.Stock)
		case "image_id":
			response["ok"] = productrules.ValidImageID(request.Value)
		case "image_path":
			response["ok"] = productrules.ValidImagePath(request.Value)
		case "plain_text":
			response["ok"] = productrules.PlainText(request.Value)
		case "normalize":
			normalized, problems := productrules.NormalizeAttributes(request.Attributes)
			response["attributes"] = normalized
			response["problems"] = outProblems(problems)
		case "validate":
			response["problems"] = outProblems(productrules.ValidateAttributes(request.productType(), request.Attributes))
		default:
			response["error"] = fmt.Sprintf("unknown op %q", request.Op)
		}
		_ = encoder.Encode(response)
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
