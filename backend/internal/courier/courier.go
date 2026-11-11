package courier

import (
	"bytes"
	"context"
	_ "embed"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"
)

var (
	ErrUnknownWilaya  = errors.New("unknown wilaya")
	ErrUnknownCommune = errors.New("unknown commune")
	ErrUnknownDesk    = errors.New("unknown stop desk")
	ErrTransport      = errors.New("courier transport")
)

//go:embed data/wilayas.json
var wilayaSnapshot []byte

//go:embed data/communes.json
var communeSnapshot []byte

var (
	fallbackWilayas  = sync.OnceValue(loadWilayaSnapshot)
	fallbackCommunes = sync.OnceValue(loadCommuneSnapshot)
)

func loadWilayaSnapshot() []Wilaya {
	var value []Wilaya
	if err := json.Unmarshal(wilayaSnapshot, &value); err != nil {
		return nil
	}
	return value
}

func loadCommuneSnapshot() []Commune {
	var value []Commune
	if err := json.Unmarshal(communeSnapshot, &value); err != nil {
		return nil
	}
	return value
}

type Wilaya struct {
	Code             int    `json:"code"`
	NameAr           string `json:"nameAr"`
	NameFr           string `json:"nameFr"`
	IsDeepSouth      bool   `json:"isDeepSouth"`
	CommuneCount     int    `json:"communeCount"`
	CourierSupported bool   `json:"courierSupported"`
}

type Commune struct {
	WilayaCode int    `json:"wilayaCode"`
	NameFr     string `json:"nameFr"`
	NameAr     string `json:"nameAr"`
}

type Desk struct {
	ID          string `json:"id"`
	Name        string `json:"name"`
	WilayaCode  int    `json:"wilayaCode"`
	CommuneName string `json:"communeName"`
	Address     string `json:"address"`
}

type Shipment struct {
	TrackingNumber   string `json:"trackingNumber"`
	CourierReference string `json:"courierReference"`
	Status           string `json:"status"`
	Reference        string `json:"reference"`
}

type TrackingEvent struct {
	Status    string `json:"status"`
	RawStatus string `json:"rawStatus"`
	Timestamp string `json:"timestamp"`
}

type Tracking struct {
	TrackingNumber string          `json:"trackingNumber"`
	Status         string          `json:"status"`
	Events         []TrackingEvent `json:"events"`
}

type Recipient struct {
	FullName    string
	Phone       string
	PhoneAlt    string
	WilayaCode  int
	CommuneName string
	AddressLine string
}

type OrderRequest struct {
	Reference    string
	Recipient    Recipient
	DeliveryType string
	StopDeskID   string
	ProductList  string
	CODAmount    int64
	FreeShipping bool
	Notes        string
}

type APIError struct {
	Status     int
	Code       string
	Message    string
	Fields     map[string]string
	RetryAfter time.Duration
}

func (e *APIError) Error() string {
	if e.Message != "" {
		return e.Message
	}
	return fmt.Sprintf("courier returned HTTP %d", e.Status)
}

type Options struct {
	BaseURL           string
	CourierKey        string
	Credentials       map[string]string
	FromWilaya        int
	Timeout           time.Duration
	CacheTTL          time.Duration
	AllowLocalSandbox bool
	OnFallback        func(resource string)
}

type Client struct {
	baseURL      string
	courierKey   string
	credentials  map[string]string
	fromWilaya   int
	http         *http.Client
	ttl          time.Duration
	now          func() time.Time
	localSandbox bool
	onFallback   func(string)

	mu           sync.Mutex
	wilayas      cache[[]Wilaya]
	communes     map[int]*cache[[]Commune]
	desks        map[int]*cache[[]Desk]
	offlineUntil time.Time
}

type cache[T any] struct {
	mu      sync.Mutex
	value   T
	expires time.Time
	has     bool
}

func (c *cache[T]) get(now time.Time) (T, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.value, c.has && now.Before(c.expires)
}

func (c *cache[T]) stale() (T, bool) {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.value, c.has
}

func (c *cache[T]) set(value T, expires time.Time) {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.value = value
	c.expires = expires
	c.has = true
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

func New(opts Options) *Client {
	timeout := opts.Timeout
	if timeout <= 0 {
		timeout = 12 * time.Second
	}
	localSandbox := opts.AllowLocalSandbox && strings.TrimSpace(opts.CourierKey) == "sandbox"
	// A sandbox demo must not hold the admin for the full production timeout
	// when the host has no outbound route.
	if localSandbox && timeout > 4*time.Second {
		timeout = 4 * time.Second
	}
	ttl := opts.CacheTTL
	if ttl <= 0 {
		ttl = 24 * time.Hour
	}
	return &Client{
		baseURL:      strings.TrimRight(strings.TrimSpace(opts.BaseURL), "/"),
		courierKey:   strings.TrimSpace(opts.CourierKey),
		credentials:  opts.Credentials,
		fromWilaya:   opts.FromWilaya,
		localSandbox: localSandbox,
		onFallback:   opts.OnFallback,
		http: &http.Client{
			Timeout: timeout,
			CheckRedirect: func(_ *http.Request, _ []*http.Request) error {
				return http.ErrUseLastResponse
			},
		},
		ttl:      ttl,
		now:      time.Now,
		communes: make(map[int]*cache[[]Commune]),
		desks:    make(map[int]*cache[[]Desk]),
	}
}

func (c *Client) Wilayas(ctx context.Context) ([]Wilaya, error) {
	now := c.now()
	if value, fresh := c.wilayas.get(now); fresh {
		return value, nil
	}
	if c.localSandbox && c.isOffline() {
		if snapshot := fallbackWilayas(); len(snapshot) > 0 {
			c.notifyFallback("wilayas")
			c.wilayas.set(snapshot, c.now().Add(5*time.Minute))
			return snapshot, nil
		}
	}

	var value []Wilaya
	if err := c.doJSON(ctx, http.MethodGet, "/v1/wilayas", nil, &value); err != nil {
		if stale, ok := c.wilayas.stale(); ok {
			return stale, nil
		}
		if snapshot := fallbackWilayas(); len(snapshot) > 0 {
			c.markOffline()
			c.notifyFallback("wilayas")
			c.wilayas.set(snapshot, c.now().Add(5*time.Minute))
			return snapshot, nil
		}
		return nil, err
	}
	if value == nil {
		value = []Wilaya{}
	}
	c.wilayas.set(value, c.now().Add(c.ttl))
	return value, nil
}

func (c *Client) Wilaya(ctx context.Context, code int) (Wilaya, error) {
	wilayas, err := c.Wilayas(ctx)
	if err != nil {
		return Wilaya{}, err
	}
	for _, wilaya := range wilayas {
		if wilaya.Code == code {
			return wilaya, nil
		}
	}
	return Wilaya{}, ErrUnknownWilaya
}

func (c *Client) Communes(ctx context.Context, wilayaCode int) ([]Commune, error) {
	entry := c.communeCache(wilayaCode)
	now := c.now()
	if value, fresh := entry.get(now); fresh {
		return value, nil
	}
	if c.localSandbox && c.isOffline() {
		if snapshot := fallbackCommunesFor(wilayaCode); len(snapshot) > 0 {
			c.notifyFallback("communes")
			entry.set(snapshot, c.now().Add(5*time.Minute))
			return snapshot, nil
		}
	}

	path := "/v1/communes?wilaya=" + strconv.Itoa(wilayaCode)
	var value []Commune
	if err := c.doJSON(ctx, http.MethodGet, path, nil, &value); err != nil {
		if stale, ok := entry.stale(); ok {
			return stale, nil
		}
		if snapshot := fallbackCommunesFor(wilayaCode); len(snapshot) > 0 {
			c.markOffline()
			c.notifyFallback("communes")
			entry.set(snapshot, c.now().Add(5*time.Minute))
			return snapshot, nil
		}
		return nil, err
	}
	if value == nil {
		value = []Commune{}
	}
	entry.set(value, c.now().Add(c.ttl))
	return value, nil
}

func (c *Client) Commune(ctx context.Context, wilayaCode int, name string) (string, error) {
	communes, err := c.Communes(ctx, wilayaCode)
	if err != nil {
		return "", err
	}
	needle := strings.TrimSpace(name)
	for _, commune := range communes {
		if strings.EqualFold(commune.NameFr, needle) {
			return commune.NameFr, nil
		}
	}
	return "", ErrUnknownCommune
}

func (c *Client) Desks(ctx context.Context, wilayaCode int) ([]Desk, error) {
	entry := c.deskCache(wilayaCode)
	now := c.now()
	if value, fresh := entry.get(now); fresh {
		return value, nil
	}
	if c.localSandbox && c.isOffline() {
		value := c.localDesks(wilayaCode)
		c.notifyFallback("desks")
		entry.set(value, c.now().Add(5*time.Minute))
		return value, nil
	}

	body := desksPayload{
		Courier:     c.courierKey,
		Credentials: c.credentialsOrNil(),
		Options:     c.options(),
		WilayaCode:  wilayaCode,
	}
	var value []Desk
	if err := c.doJSON(ctx, http.MethodPost, "/v1/desks", body, &value); err != nil {
		if stale, ok := entry.stale(); ok {
			return stale, nil
		}
		if c.localSandbox && isUnavailable(err) {
			c.markOffline()
			value = c.localDesks(wilayaCode)
			c.notifyFallback("desks")
			entry.set(value, c.now().Add(5*time.Minute))
			return value, nil
		}
		return nil, err
	}
	if value == nil {
		value = []Desk{}
	}
	entry.set(value, c.now().Add(c.ttl))
	return value, nil
}

func (c *Client) Desk(ctx context.Context, wilayaCode int, id string) (Desk, error) {
	desks, err := c.Desks(ctx, wilayaCode)
	if err != nil {
		return Desk{}, err
	}
	for _, desk := range desks {
		if desk.ID == id {
			return desk, nil
		}
	}
	return Desk{}, ErrUnknownDesk
}

func (c *Client) CreateOrder(ctx context.Context, req OrderRequest) (Shipment, error) {
	if c.localSandbox && c.isOffline() {
		c.notifyFallback("orders")
		return localShipment(req), nil
	}

	body := createOrderPayload{
		Courier:     c.courierKey,
		Credentials: c.credentialsOrNil(),
		Options:     c.options(),
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
	if err := c.doJSON(ctx, http.MethodPost, "/v1/orders", body, &shipment); err != nil {
		if c.localSandbox && isUnavailable(err) {
			c.markOffline()
			c.notifyFallback("orders")
			return localShipment(req), nil
		}
		return Shipment{}, err
	}
	if strings.TrimSpace(shipment.TrackingNumber) == "" {
		return Shipment{}, &APIError{
			Status:  http.StatusBadGateway,
			Code:    "courier_error",
			Message: "The courier did not return a tracking number.",
		}
	}
	return shipment, nil
}

func (c *Client) Track(ctx context.Context, courierKey, trackingNumber string) (Tracking, error) {
	if c.localSandbox && c.isOffline() {
		c.notifyFallback("track")
		return c.localTracking(trackingNumber), nil
	}

	key := strings.TrimSpace(courierKey)
	if key == "" {
		key = c.courierKey
	}
	body := trackPayload{
		Courier:        key,
		Credentials:    c.credentialsOrNil(),
		Options:        c.options(),
		TrackingNumber: trackingNumber,
	}

	var tracking Tracking
	if err := c.doJSON(ctx, http.MethodPost, "/v1/track", body, &tracking); err != nil {
		if c.localSandbox && isUnavailable(err) {
			c.markOffline()
			c.notifyFallback("track")
			return c.localTracking(trackingNumber), nil
		}
		return Tracking{}, err
	}
	return tracking, nil
}

func (c *Client) communeCache(wilayaCode int) *cache[[]Commune] {
	c.mu.Lock()
	defer c.mu.Unlock()
	entry, ok := c.communes[wilayaCode]
	if !ok {
		entry = &cache[[]Commune]{}
		c.communes[wilayaCode] = entry
	}
	return entry
}

func (c *Client) deskCache(wilayaCode int) *cache[[]Desk] {
	c.mu.Lock()
	defer c.mu.Unlock()
	entry, ok := c.desks[wilayaCode]
	if !ok {
		entry = &cache[[]Desk]{}
		c.desks[wilayaCode] = entry
	}
	return entry
}

func (c *Client) credentialsOrNil() map[string]string {
	if len(c.credentials) == 0 {
		return nil
	}
	return c.credentials
}

func (c *Client) options() *optionsPayload {
	if c.fromWilaya == 0 {
		return nil
	}
	return &optionsPayload{FromWilaya: c.fromWilaya}
}

func (c *Client) doJSON(ctx context.Context, method, path string, body, out any) error {
	var reader io.Reader
	if body != nil {
		encoded, err := json.Marshal(body)
		if err != nil {
			return fmt.Errorf("encode courier request: %w", err)
		}
		reader = bytes.NewReader(encoded)
	}

	req, err := http.NewRequestWithContext(ctx, method, c.baseURL+path, reader)
	if err != nil {
		return fmt.Errorf("build courier request: %w", err)
	}
	req.Header.Set("Accept", "application/json")
	req.Header.Set("User-Agent", "azahar-store/1.0")
	if body != nil {
		req.Header.Set("Content-Type", "application/json")
	}

	resp, err := c.http.Do(req)
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

func (c *Client) notifyFallback(resource string) {
	if c.onFallback != nil {
		c.onFallback(resource)
	}
}

func (c *Client) isOffline() bool {
	if !c.localSandbox {
		return false
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.now().Before(c.offlineUntil)
}

func (c *Client) markOffline() {
	if !c.localSandbox {
		return
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	c.offlineUntil = c.now().Add(2 * time.Minute)
}

func isUnavailable(err error) bool {
	if errors.Is(err, ErrTransport) {
		return true
	}
	var apiErr *APIError
	if errors.As(err, &apiErr) {
		return apiErr.Status >= http.StatusInternalServerError
	}
	return false
}

func fallbackCommunesFor(wilayaCode int) []Commune {
	all := fallbackCommunes()
	result := make([]Commune, 0)
	for _, commune := range all {
		if commune.WilayaCode == wilayaCode {
			result = append(result, commune)
		}
	}
	return result
}

func (c *Client) localDesks(wilayaCode int) []Desk {
	wilayaName := fmt.Sprintf("Wilaya %d", wilayaCode)
	for _, wilaya := range fallbackWilayas() {
		if wilaya.Code == wilayaCode {
			wilayaName = wilaya.NameFr
			break
		}
	}

	commune := ""
	for _, candidate := range fallbackCommunesFor(wilayaCode) {
		if candidate.NameFr != "" {
			commune = candidate.NameFr
			break
		}
	}

	address := "Sandbox address"
	if commune != "" {
		address = "Sandbox address, " + commune
	}
	return []Desk{{
		ID:          fmt.Sprintf("SBX-%d", wilayaCode),
		Name:        "Sandbox desk " + wilayaName,
		WilayaCode:  wilayaCode,
		CommuneName: commune,
		Address:     address,
	}}
}

func localShipment(req OrderRequest) Shipment {
	tracking := "SBX-" + sanitizeTracking(req.Reference)
	if tracking == "SBX-" {
		tracking = "SBX-LOCAL"
	}
	return Shipment{
		TrackingNumber: tracking,
		Status:         "created",
		Reference:      req.Reference,
	}
}

func sanitizeTracking(value string) string {
	var b strings.Builder
	for _, r := range strings.ToUpper(strings.TrimSpace(value)) {
		if (r >= 'A' && r <= 'Z') || (r >= '0' && r <= '9') || r == '-' {
			b.WriteRune(r)
		}
		if b.Len() >= 60 {
			break
		}
	}
	return b.String()
}

func (c *Client) localTracking(trackingNumber string) Tracking {
	now := c.now().UTC()
	return Tracking{
		TrackingNumber: trackingNumber,
		Status:         "in_transit",
		Events: []TrackingEvent{
			{Status: "created", RawStatus: "Créé (sandbox local)", Timestamp: now.Add(-2 * time.Hour).Format(time.RFC3339)},
			{Status: "picked_up", RawStatus: "Ramassé (sandbox local)", Timestamp: now.Add(-time.Hour).Format(time.RFC3339)},
			{Status: "in_transit", RawStatus: "En transit (sandbox local)", Timestamp: now.Format(time.RFC3339)},
		},
	}
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
