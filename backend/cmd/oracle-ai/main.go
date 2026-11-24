//go:build oracle

// Command oracle-ai is the line-oriented differential oracle for the Rust port
// of internal/plugins/ai. It exposes the settings configuration, prompt
// builders, DZD formatting, manifest/migrations/routes metadata and the
// UpstreamError mapping; the model itself is not called.
//
//	go run -tags oracle ./cmd/oracle-ai
package main

import (
	"bufio"
	"encoding/json"
	"fmt"
	"os"
	"time"

	"azahar/internal/plugins/ai"
	"azahar/internal/store"
)

type request struct {
	Op         string            `json:"op"`
	Language   string            `json:"language"`
	Days       int               `json:"days"`
	Cents      int64             `json:"cents"`
	Metrics    *wireMetrics      `json:"metrics"`
	Product    *wireProduct      `json:"product"`
	Sales      *wireSales        `json:"sales"`
	Settings   map[string]string `json:"settings"`
	StatusCode int               `json:"status_code"`
	Reason     string            `json:"reason"`
}

type wireMetrics struct {
	From                 string           `json:"from"`
	To                   string           `json:"to"`
	RevenueCents         int64            `json:"revenue_cents"`
	PreviousRevenueCents int64            `json:"previous_revenue_cents"`
	OrdersCount          int              `json:"orders_count"`
	UnitsSold            int              `json:"units_sold"`
	NewCustomers         int              `json:"new_customers"`
	TopProducts          []wireTopProduct `json:"top_products"`
	LowStock             []wireLowStock   `json:"low_stock"`
	StatusSplit          map[string]int   `json:"status_split"`
}

type wireTopProduct struct {
	Name         string `json:"name"`
	Units        int    `json:"units"`
	RevenueCents int64  `json:"revenue_cents"`
}

type wireLowStock struct {
	Name  string `json:"name"`
	Stock int    `json:"stock"`
}

type wireProduct struct {
	Name         string              `json:"name"`
	Subtitle     string              `json:"subtitle"`
	Description  string              `json:"description"`
	PriceCents   int64               `json:"price_cents"`
	Size         string              `json:"size"`
	Stock        int                 `json:"stock"`
	CategoryName string              `json:"category_name"`
	Concerns     []string            `json:"concerns"`
	Ingredients  []string            `json:"ingredients"`
	Attributes   map[string][]string `json:"attributes"`
}

type wireSales struct {
	Days         int   `json:"days"`
	Units        int   `json:"units"`
	Orders       int   `json:"orders"`
	RevenueCents int64 `json:"revenue_cents"`
}

func main() {
	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 0, 64*1024), 8<<20)
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
		writeJSON(writer, handle(req))
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-ai: read:", err)
		os.Exit(1)
	}
}

func handle(req request) map[string]any {
	switch req.Op {
	case "store_prompt":
		metrics, err := toMetrics(req.Metrics)
		if err != nil {
			return map[string]any{"error": err.Error()}
		}
		system, user := ai.OracleBuildStorePrompt(req.Language, req.Days, metrics)
		return map[string]any{"system": system, "user": user}
	case "product_prompt":
		if req.Product == nil {
			return map[string]any{"error": "product is required"}
		}
		product := store.Product{
			Name:        req.Product.Name,
			Subtitle:    req.Product.Subtitle,
			Description: req.Product.Description,
			PriceCents:  req.Product.PriceCents,
			Size:        req.Product.Size,
			Stock:       req.Product.Stock,
			Category:    store.Category{Name: req.Product.CategoryName},
			Concerns:    req.Product.Concerns,
			Ingredients: req.Product.Ingredients,
			Attributes:  req.Product.Attributes,
		}
		sales := wireSales{}
		if req.Sales != nil {
			sales = *req.Sales
		}
		system, user := ai.OracleBuildProductPrompt(req.Language, product,
			ai.OracleProductSales(sales.Days, sales.Units, sales.Orders, sales.RevenueCents))
		return map[string]any{"system": system, "user": user}
	case "instruction":
		return map[string]any{"instruction": ai.OracleAnalysisInstruction(req.Language)}
	case "format_dzd":
		return map[string]any{"formatted": ai.OracleFormatDZD(req.Cents)}
	case "settings":
		config, class := ai.OracleConfig(req.Settings)
		response := map[string]any{
			"enabled": ai.OracleEnabled(req.Settings),
			"class":   class,
		}
		if class == "" {
			response["config"] = config
		}
		return response
	case "manifest":
		return map[string]any{"manifest": ai.New().Manifest()}
	case "migrations":
		return map[string]any{"migrations": ai.New().Migrations()}
	case "routes":
		routes := make([]map[string]any, 0, 2)
		for _, route := range ai.New().Routes() {
			routes = append(routes, map[string]any{
				"Method":  route.Method,
				"Pattern": route.Pattern,
				"Admin":   route.Admin,
			})
		}
		return map[string]any{"routes": routes}
	case "validate":
		return map[string]any{"fields": ai.New().ValidateSettings(req.Settings)}
	case "upstream_error":
		upstream := &ai.UpstreamError{StatusCode: req.StatusCode, Reason: req.Reason}
		return map[string]any{
			"error":        upstream.Error(),
			"status_class": upstream.StatusClass(),
			"detail":       upstream.Detail(),
		}
	default:
		return map[string]any{"error": fmt.Sprintf("unknown op %q", req.Op)}
	}
}

func toMetrics(wire *wireMetrics) (store.Metrics, error) {
	if wire == nil {
		return store.Metrics{}, fmt.Errorf("metrics are required")
	}
	from, err := time.Parse(time.RFC3339, wire.From)
	if err != nil {
		return store.Metrics{}, fmt.Errorf("from: %w", err)
	}
	to, err := time.Parse(time.RFC3339, wire.To)
	if err != nil {
		return store.Metrics{}, fmt.Errorf("to: %w", err)
	}
	metrics := store.Metrics{
		From:                 from,
		To:                   to,
		RevenueCents:         wire.RevenueCents,
		PreviousRevenueCents: wire.PreviousRevenueCents,
		OrdersCount:          wire.OrdersCount,
		UnitsSold:            wire.UnitsSold,
		NewCustomers:         wire.NewCustomers,
		LowStock:             make([]store.LowStockItem, 0, len(wire.LowStock)),
		StatusSplit:          wire.StatusSplit,
		TopProducts:          make([]store.TopProduct, 0, len(wire.TopProducts)),
	}
	if metrics.StatusSplit == nil {
		metrics.StatusSplit = map[string]int{}
	}
	for _, item := range wire.TopProducts {
		metrics.TopProducts = append(metrics.TopProducts, store.TopProduct{
			Name:         item.Name,
			Units:        item.Units,
			RevenueCents: item.RevenueCents,
		})
	}
	for _, item := range wire.LowStock {
		metrics.LowStock = append(metrics.LowStock, store.LowStockItem{
			Name:  item.Name,
			Stock: item.Stock,
		})
	}
	return metrics, nil
}

func writeJSON(writer *bufio.Writer, value any) {
	raw, err := json.Marshal(value)
	if err != nil {
		raw, err = json.Marshal(map[string]string{"error": err.Error()})
		if err != nil {
			fmt.Fprintln(os.Stderr, "oracle-ai: encode:", err)
			os.Exit(1)
		}
	}
	writer.Write(raw)
	writer.WriteByte('\n')
	if err := writer.Flush(); err != nil {
		fmt.Fprintln(os.Stderr, "oracle-ai: flush:", err)
		os.Exit(1)
	}
}
