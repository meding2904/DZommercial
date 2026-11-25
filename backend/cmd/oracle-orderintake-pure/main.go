// Command oracle-orderintake-pure exposes the pure order-intake helpers for
// Rust differential tests.
//
//	go run ./cmd/oracle-orderintake-pure
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"

	"azahar/internal/orderintake"
)

type linePayload struct {
	ProductID          string            `json:"product_id"`
	Quantity           int               `json:"quantity"`
	Options            map[string]string `json:"options"`
	ExpectedPriceCents *int64            `json:"expected_price_cents"`
}

type lineResult struct {
	ProductID          string            `json:"product_id"`
	Quantity           int               `json:"quantity"`
	Options            map[string]string `json:"options"`
	ExpectedPriceCents *int64            `json:"expected_price_cents"`
}

type request struct {
	Op            string        `json:"op"`
	SubtotalCents int64         `json:"subtotal_cents"`
	Items         []linePayload `json:"items"`
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
		switch request.Op {
		case "shipping":
			_ = encoder.Encode(map[string]any{"cents": orderintake.ShippingCents(request.SubtotalCents)})
		case "no_shipping":
			_ = encoder.Encode(map[string]any{"cents": orderintake.NoShipping(request.SubtotalCents)})
		case "build_lines":
			lines := make([]orderintake.Line, 0, len(request.Items))
			for _, item := range request.Items {
				lines = append(lines, orderintake.Line{
					ProductID:          item.ProductID,
					Quantity:           item.Quantity,
					Options:            item.Options,
					ExpectedPriceCents: item.ExpectedPriceCents,
				})
			}
			built, fields := orderintake.BuildLines(lines)
			result := make([]lineResult, 0, len(built))
			for _, line := range built {
				result = append(result, lineResult{
					ProductID:          line.ProductID,
					Quantity:           line.Quantity,
					Options:            line.Options,
					ExpectedPriceCents: line.ExpectedPriceCents,
				})
			}
			_ = encoder.Encode(map[string]any{"lines": result, "fields": fields})
		default:
			_ = encoder.Encode(map[string]any{"error": fmt.Sprintf("unknown op %q", request.Op)})
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}
