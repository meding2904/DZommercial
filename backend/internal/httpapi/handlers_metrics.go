package httpapi

import (
	"math"
	"net/http"
	"strconv"
	"time"

	"azahar/internal/store"
)

type metricsRangeDTO struct {
	Days int    `json:"days"`
	From string `json:"from"`
	To   string `json:"to"`
}

type lowStockDTO struct {
	ID    string `json:"id"`
	Name  string `json:"name"`
	Stock int    `json:"stock"`
	Image string `json:"image"`
}

type revenuePointDTO struct {
	Date         string `json:"date"`
	RevenueCents int64  `json:"revenue_cents"`
	Orders       int    `json:"orders"`
}

type statusCountDTO struct {
	Status string `json:"status"`
	Count  int    `json:"count"`
}

type topProductDTO struct {
	ProductID    string `json:"product_id"`
	Name         string `json:"name"`
	Units        int    `json:"units"`
	RevenueCents int64  `json:"revenue_cents"`
}

type recentOrderDTO struct {
	ID         string `json:"id"`
	Number     string `json:"number"`
	Name       string `json:"name"`
	TotalCents int64  `json:"total_cents"`
	Status     string `json:"status"`
	CreatedAt  string `json:"created_at"`
}

type metricsResponse struct {
	Demo                 bool              `json:"demo"`
	Range                metricsRangeDTO   `json:"range"`
	RevenueCents         int64             `json:"revenue_cents"`
	PreviousRevenueCents int64             `json:"previous_revenue_cents"`
	DeltaPct             *float64          `json:"delta_pct"`
	OrdersCount          int               `json:"orders_count"`
	PreviousOrdersCount  int               `json:"previous_orders_count"`
	AOVCents             int64             `json:"aov_cents"`
	UnitsSold            int               `json:"units_sold"`
	NewCustomers         int               `json:"new_customers"`
	LowStock             []lowStockDTO     `json:"low_stock"`
	RevenueSeries        []revenuePointDTO `json:"revenue_series"`
	StatusSplit          []statusCountDTO  `json:"status_split"`
	TopProducts          []topProductDTO   `json:"top_products"`
	RecentOrders         []recentOrderDTO  `json:"recent_orders"`
}

func (s *Server) handleMetrics(w http.ResponseWriter, r *http.Request) {
	days := 30
	if raw := r.URL.Query().Get("days"); raw != "" {
		parsed, err := strconv.Atoi(raw)
		if err != nil || (parsed != 7 && parsed != 30 && parsed != 90) {
			writeProblem(w, problemFields(422, "validation_error", "Check the marked fields.",
				map[string]string{"days": "Choose 7, 30 or 90."}))
			return
		}
		days = parsed
	}

	now := time.Now().UTC()
	to := time.Date(now.Year(), now.Month(), now.Day(), 0, 0, 0, 0, time.UTC)
	from := to.AddDate(0, 0, -(days - 1))
	previousTo := from.AddDate(0, 0, -1)
	previousFrom := previousTo.AddDate(0, 0, -(days - 1))

	metrics, err := s.store.Metrics(r.Context(), from, to, previousFrom, previousTo)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	var delta *float64
	if metrics.PreviousRevenueCents > 0 {
		change := (float64(metrics.RevenueCents) - float64(metrics.PreviousRevenueCents)) /
			float64(metrics.PreviousRevenueCents) * 100
		rounded := math.Round(change*10) / 10
		delta = &rounded
	}
	var aov int64
	if metrics.OrdersCount > 0 {
		aov = metrics.RevenueCents / int64(metrics.OrdersCount)
	}

	response := metricsResponse{
		Demo:                 metrics.Demo,
		Range:                metricsRangeDTO{Days: days, From: from.Format("2006-01-02"), To: to.Format("2006-01-02")},
		RevenueCents:         metrics.RevenueCents,
		PreviousRevenueCents: metrics.PreviousRevenueCents,
		DeltaPct:             delta,
		OrdersCount:          metrics.OrdersCount,
		PreviousOrdersCount:  metrics.PreviousOrdersCount,
		AOVCents:             aov,
		UnitsSold:            metrics.UnitsSold,
		NewCustomers:         metrics.NewCustomers,
		LowStock:             make([]lowStockDTO, 0, len(metrics.LowStock)),
		RevenueSeries:        make([]revenuePointDTO, 0, len(metrics.RevenueSeries)),
		StatusSplit:          make([]statusCountDTO, 0, len(orderStatuses)),
		TopProducts:          make([]topProductDTO, 0, len(metrics.TopProducts)),
		RecentOrders:         make([]recentOrderDTO, 0, len(metrics.RecentOrders)),
	}
	for _, item := range metrics.LowStock {
		response.LowStock = append(response.LowStock, lowStockDTO{
			ID: item.ID, Name: item.Name, Stock: item.Stock, Image: item.Image,
		})
	}
	for _, point := range metrics.RevenueSeries {
		response.RevenueSeries = append(response.RevenueSeries, revenuePointDTO{
			Date: point.Date, RevenueCents: point.RevenueCents, Orders: point.Orders,
		})
	}
	for _, status := range orderStatuses {
		response.StatusSplit = append(response.StatusSplit, statusCountDTO{
			Status: status, Count: metrics.StatusSplit[status],
		})
	}
	for _, product := range metrics.TopProducts {
		response.TopProducts = append(response.TopProducts, topProductDTO{
			ProductID: product.ProductID, Name: product.Name,
			Units: product.Units, RevenueCents: product.RevenueCents,
		})
	}
	for _, order := range metrics.RecentOrders {
		response.RecentOrders = append(response.RecentOrders, recentOrderDTO{
			ID: order.ID, Number: order.Number, Name: order.Name,
			TotalCents: order.TotalCents, Status: order.Status,
			CreatedAt: store.FormatTime(order.CreatedAt),
		})
	}

	writeJSON(w, http.StatusOK, response)
}
