// Command oracle-courier is the differential oracle for the Rust courier port.
// It reads one JSON command per line from stdin and writes one JSON result per
// line to stdout. It never talks to the real courier service: the default
// base_url is an unreachable local port, which makes the bundled fallback and
// the sandbox deterministic.
package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"strings"
	"time"

	"azahar/internal/courier"
)

type wireOptions struct {
	BaseURL           *string `json:"base_url"`
	CourierKey        *string `json:"courier_key"`
	AllowLocalSandbox *bool   `json:"allow_local_sandbox"`
	FromWilaya        *int    `json:"from_wilaya"`
	TimeoutMS         *int    `json:"timeout_ms"`
	CacheTTLMS        *int    `json:"cache_ttl_ms"`
}

type wireRecipient struct {
	FullName    string `json:"full_name"`
	Phone       string `json:"phone"`
	PhoneAlt    string `json:"phone_alt"`
	WilayaCode  int    `json:"wilaya_code"`
	CommuneName string `json:"commune_name"`
	AddressLine string `json:"address_line"`
}

type command struct {
	Op             string        `json:"op"`
	Code           int           `json:"code"`
	Wilaya         int           `json:"wilaya"`
	Name           string        `json:"name"`
	ID             string        `json:"id"`
	TrackingNumber string        `json:"tracking_number"`
	Reference      string        `json:"reference"`
	Recipient      wireRecipient `json:"recipient"`
	DeliveryType   string        `json:"delivery_type"`
	StopDeskID     string        `json:"stop_desk_id"`
	ProductList    string        `json:"product_list"`
	CODAmount      int64         `json:"cod_amount"`
	FreeShipping   bool          `json:"free_shipping"`
	Notes          string        `json:"notes"`
	Options        wireOptions   `json:"options"`

	BaseURL           *string `json:"base_url"`
	CourierKey        *string `json:"courier_key"`
	AllowLocalSandbox *bool   `json:"allow_local_sandbox"`
	FromWilaya        *int    `json:"from_wilaya"`
	TimeoutMS         *int    `json:"timeout_ms"`
	CacheTTLMS        *int    `json:"cache_ttl_ms"`
}

type errorBody struct {
	Kind       string            `json:"kind"`
	Message    string            `json:"message"`
	Status     int               `json:"status,omitempty"`
	Code       string            `json:"code,omitempty"`
	Fields     map[string]string `json:"fields,omitempty"`
	RetryAfter int64             `json:"retry_after_ms,omitempty"`
}

type response struct {
	OK        bool       `json:"ok"`
	Value     any        `json:"value,omitempty"`
	Error     *errorBody `json:"error,omitempty"`
	Fallbacks []string   `json:"fallbacks"`
}

func main() {
	encoder := json.NewEncoder(os.Stdout)
	encoder.SetEscapeHTML(false)

	scanner := bufio.NewScanner(os.Stdin)
	scanner.Buffer(make([]byte, 64*1024), 4*1024*1024)
	for scanner.Scan() {
		line := strings.TrimSpace(scanner.Text())
		if line == "" {
			continue
		}
		var cmd command
		if err := json.Unmarshal([]byte(line), &cmd); err != nil {
			_ = encoder.Encode(response{
				Error:     &errorBody{Kind: "usage", Message: "decode command: " + err.Error()},
				Fallbacks: []string{},
			})
			continue
		}
		if err := encoder.Encode(run(cmd)); err != nil {
			fmt.Fprintln(os.Stderr, "write response:", err)
			os.Exit(1)
		}
	}
	if err := scanner.Err(); err != nil {
		fmt.Fprintln(os.Stderr, "read stdin:", err)
		os.Exit(1)
	}
}

func (c command) clientOptions() courier.Options {
	baseURL := "http://127.0.0.1:1"
	courierKey := "sandbox"
	allowLocalSandbox := true
	fromWilaya := 16
	timeoutMS := 0
	cacheTTLMS := 0

	apply := func(o wireOptions) {
		if o.BaseURL != nil {
			baseURL = *o.BaseURL
		}
		if o.CourierKey != nil {
			courierKey = *o.CourierKey
		}
		if o.AllowLocalSandbox != nil {
			allowLocalSandbox = *o.AllowLocalSandbox
		}
		if o.FromWilaya != nil {
			fromWilaya = *o.FromWilaya
		}
		if o.TimeoutMS != nil {
			timeoutMS = *o.TimeoutMS
		}
		if o.CacheTTLMS != nil {
			cacheTTLMS = *o.CacheTTLMS
		}
	}

	apply(wireOptions{
		BaseURL:           c.BaseURL,
		CourierKey:        c.CourierKey,
		AllowLocalSandbox: c.AllowLocalSandbox,
		FromWilaya:        c.FromWilaya,
		TimeoutMS:         c.TimeoutMS,
		CacheTTLMS:        c.CacheTTLMS,
	})
	apply(c.Options)

	return courier.Options{
		BaseURL:           baseURL,
		CourierKey:        courierKey,
		FromWilaya:        fromWilaya,
		Timeout:           time.Duration(timeoutMS) * time.Millisecond,
		CacheTTL:          time.Duration(cacheTTLMS) * time.Millisecond,
		AllowLocalSandbox: allowLocalSandbox,
	}
}

func run(cmd command) response {
	fallbacks := []string{}
	options := cmd.clientOptions()
	options.OnFallback = func(resource string) {
		fallbacks = append(fallbacks, resource)
	}
	client := courier.New(options)

	ctx, cancel := context.WithTimeout(context.Background(), 30*time.Second)
	defer cancel()

	var value any
	var err error
	switch cmd.Op {
	case "wilayas":
		value, err = client.Wilayas(ctx)
	case "wilaya":
		value, err = client.Wilaya(ctx, cmd.Code)
	case "communes":
		value, err = client.Communes(ctx, cmd.Wilaya)
	case "commune":
		value, err = client.Commune(ctx, cmd.Wilaya, cmd.Name)
	case "desks":
		value, err = client.Desks(ctx, cmd.Wilaya)
	case "desk":
		value, err = client.Desk(ctx, cmd.Wilaya, cmd.ID)
	case "create_order":
		value, err = client.CreateOrder(ctx, courier.OrderRequest{
			Reference: cmd.Reference,
			Recipient: courier.Recipient{
				FullName:    cmd.Recipient.FullName,
				Phone:       cmd.Recipient.Phone,
				PhoneAlt:    cmd.Recipient.PhoneAlt,
				WilayaCode:  cmd.Recipient.WilayaCode,
				CommuneName: cmd.Recipient.CommuneName,
				AddressLine: cmd.Recipient.AddressLine,
			},
			DeliveryType: cmd.DeliveryType,
			StopDeskID:   cmd.StopDeskID,
			ProductList:  cmd.ProductList,
			CODAmount:    cmd.CODAmount,
			FreeShipping: cmd.FreeShipping,
			Notes:        cmd.Notes,
		})
	case "track":
		value, err = client.Track(ctx, options.CourierKey, cmd.TrackingNumber)
	default:
		err = fmt.Errorf("unknown op %q", cmd.Op)
	}

	if err != nil {
		return response{Error: describe(err), Fallbacks: fallbacks}
	}
	return response{OK: true, Value: value, Fallbacks: fallbacks}
}

func describe(err error) *errorBody {
	var apiErr *courier.APIError
	if errors.As(err, &apiErr) {
		fields := apiErr.Fields
		if fields == nil {
			fields = map[string]string{}
		}
		return &errorBody{
			Kind:       "api",
			Message:    apiErr.Error(),
			Status:     apiErr.Status,
			Code:       apiErr.Code,
			Fields:     fields,
			RetryAfter: apiErr.RetryAfter.Milliseconds(),
		}
	}

	kind := "other"
	switch {
	case errors.Is(err, courier.ErrUnknownWilaya):
		kind = "unknown_wilaya"
	case errors.Is(err, courier.ErrUnknownCommune):
		kind = "unknown_commune"
	case errors.Is(err, courier.ErrUnknownDesk):
		kind = "unknown_desk"
	case errors.Is(err, courier.ErrTransport):
		kind = "transport"
	}
	return &errorBody{Kind: kind, Message: err.Error()}
}
