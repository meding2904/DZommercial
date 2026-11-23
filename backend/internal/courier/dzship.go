package courier

import (
	"bytes"
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"
)

// dzship speaks to the real courier API. It knows the wire contract and
// nothing about caching, outages or fallbacks; Client owns those.
type dzship struct {
	baseURL     string
	courierKey  string
	credentials map[string]string
	fromWilaya  int
	http        *http.Client
}

func newDZShip(opts Options, timeout time.Duration) *dzship {
	return &dzship{
		baseURL:     strings.TrimRight(strings.TrimSpace(opts.BaseURL), "/"),
		courierKey:  strings.TrimSpace(opts.CourierKey),
		credentials: opts.Credentials,
		fromWilaya:  opts.FromWilaya,
		http: &http.Client{
			Timeout: timeout,
			CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
	}
}

func (d *dzship) wilayas(ctx context.Context) ([]Wilaya, error) {
	value := []Wilaya{}
	if err := d.doJSON(ctx, http.MethodGet, "/v1/wilayas", nil, &value); err != nil {
		return nil, err
	}
	if value == nil {
		value = []Wilaya{}
	}
	return value, nil
}

func (d *dzship) communes(ctx context.Context, wilayaCode int) ([]Commune, error) {
	path := "/v1/communes?wilaya=" + strconv.Itoa(wilayaCode)
	value := []Commune{}
	if err := d.doJSON(ctx, http.MethodGet, path, nil, &value); err != nil {
		return nil, err
	}
	if value == nil {
		value = []Commune{}
	}
	return value, nil
}

func (d *dzship) desks(ctx context.Context, wilayaCode int) ([]Desk, error) {
	body := desksPayload{
		Courier:     d.courierKey,
		Credentials: d.credentialsOrNil(),
		Options:     d.options(),
		WilayaCode:  wilayaCode,
	}
	value := []Desk{}
	if err := d.doJSON(ctx, http.MethodPost, "/v1/desks", body, &value); err != nil {
		return nil, err
	}
	if value == nil {
		value = []Desk{}
	}
	return value, nil
}

func (d *dzship) createOrder(ctx context.Context, req OrderRequest) (Shipment, error) {
	body := createOrderPayload{
		Courier:     d.courierKey,
		Credentials: d.credentialsOrNil(),
		Options:     d.options(),
		Order: orderPayload{
			Reference: req.Reference,
			Recipient: recipientPayload{
				FullName:    req.Recipient.FullName,
				Phone:       req.Recipient.Phone,
				PhoneAlt:    req.Recipient.PhoneAlt,
				WilayaCode:  req.Recipient.WilayaCode,
				CommuneName: req.Recipient.CommuneName,
				AddressLine: req.Recipient.AddressLine,
			},
			DeliveryType: req.DeliveryType,
			StopDeskID:   req.StopDeskID,
			ProductList:  req.ProductList,
			CODAmount:    req.CODAmount,
			FreeShipping: req.FreeShipping,
			Notes:        req.Notes,
		},
	}

	var shipment Shipment
	if err := d.doJSON(ctx, http.MethodPost, "/v1/orders", body, &shipment); err != nil {
		return Shipment{}, err
	}
	return shipment, nil
}

func (d *dzship) track(ctx context.Context, courierKey, trackingNumber string) (Tracking, error) {
	key := strings.TrimSpace(courierKey)
	if key == "" {
		key = d.courierKey
	}
	body := trackPayload{
		Courier:        key,
		Credentials:    d.credentialsOrNil(),
		Options:        d.options(),
		TrackingNumber: trackingNumber,
	}

	var tracking Tracking
	if err := d.doJSON(ctx, http.MethodPost, "/v1/track", body, &tracking); err != nil {
		return Tracking{}, err
	}
	return tracking, nil
}

func (d *dzship) credentialsOrNil() map[string]string {
	if len(d.credentials) == 0 {
		return nil
	}
	return d.credentials
}

func (d *dzship) options() *optionsPayload {
	if d.fromWilaya == 0 {
		return nil
	}
	return &optionsPayload{FromWilaya: d.fromWilaya}
}

func (d *dzship) doJSON(ctx context.Context, method, path string, body, out any) error {
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("encode courier request: %w", err)
		}
		reader = bytes.NewReader(encoded)
	}

	req, err := http.NewRequestWithContext(ctx, method, d.baseURL+path, reader)
	if err != nil {
		return fmt.Errorf("build courier request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "azahar-store/1.0")
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}

	resp, err := d.http.Do(req)
	if err != nil {
		return fmt.Errorf("%w: %v", ErrTransport, err)
	}
	defer resp.Body.Close()

	data, err := io.ReadAll(io.LimitReader(resp.Body, 2<<20))
	if err != nil {
		return fmt.Errorf("%w: read courier response: %v", ErrTransport, err)
	}

	if resp.StatusCode < 200 || resp.StatusCode >= 300 {
		return apiError(resp, data)
	}
	if out != nil {
		if err := json.Unmarshal(data, out); err != nil {
			return fmt.Errorf("%w: decode courier response: %v", ErrTransport, err)
		}
	}
	return nil
}

type optionsPayload struct {
	FromWilaya int `json:"fromWilaya,omitempty"`
}

type recipientPayload struct {
	FullName    string `json:"fullName"`
	Phone       string `json:"phone"`
	PhoneAlt    string `json:"phoneAlt,omitempty"`
	WilayaCode  int    `json:"wilayaCode"`
	CommuneName string `json:"communeName"`
	AddressLine string `json:"addressLine,omitempty"`
}

type orderPayload struct {
	Reference    string           `json:"reference,omitempty"`
	Recipient    recipientPayload `json:"recipient"`
	DeliveryType string           `json:"deliveryType"`
	StopDeskID   string           `json:"stopDeskId,omitempty"`
	ProductList  string           `json:"productList"`
	CODAmount    int64            `json:"codAmount"`
	FreeShipping bool             `json:"freeShipping,omitempty"`
	Notes        string           `json:"notes,omitempty"`
}

type createOrderPayload struct {
	Courier     string            `json:"courier"`
	Credentials map[string]string `json:"credentials,omitempty"`
	Options     *optionsPayload   `json:"options,omitempty"`
	Order       orderPayload      `json:"order"`
}

type trackPayload struct {
	Courier        string            `json:"courier"`
	Credentials    map[string]string `json:"credentials,omitempty"`
	Options        *optionsPayload   `json:"options,omitempty"`
	TrackingNumber string            `json:"trackingNumber"`
}

type desksPayload struct {
	Courier     string            `json:"courier"`
	Credentials map[string]string `json:"credentials,omitempty"`
	Options     *optionsPayload   `json:"options,omitempty"`
	WilayaCode  int               `json:"wilayaCode"`
}

func apiError(resp *http.Response, data []byte) error {
	result := &APIError{
		Status: resp.StatusCode,
		Code:   "courier_error",
	}
	if seconds, err := strconv.Atoi(strings.TrimSpace(resp.Header.Get("Retry-After"))); err == nil && seconds > 0 {
		result.RetryAfter = time.Duration(seconds) * time.Second
	}

	var envelope struct {
		Error struct {
			Code    string            `json:"code"`
			Message string            `json:"message"`
			Fields  map[string]string `json:"fields"`
		} `json:"error"`
	}
	if err := json.Unmarshal(data, &envelope); err == nil && envelope.Error.Code != "" {
		result.Code = envelope.Error.Code
		result.Message = strings.TrimSpace(envelope.Error.Message)
		result.Fields = envelope.Error.Fields
	}
	if result.Message == "" {
		message := strings.TrimSpace(string(data))
		if message == "" || strings.HasPrefix(message, "<") {
			message = fmt.Sprintf("The courier returned HTTP %d.", resp.StatusCode)
		}
		result.Message = message
	}
	return result
}
