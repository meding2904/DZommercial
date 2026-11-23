package courier

import (
	"context"
	"errors"
	"fmt"
	"net/http"
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

// Courier is the seam the checkout and shipment flows use: the delivery map
// they offer and the parcel they hand over. Client is the production
// implementation; tests drive a fake.
type Courier interface {
	Wilayas(ctx context.Context) ([]Wilaya, error)
	Wilaya(ctx context.Context, code int) (Wilaya, error)
	Communes(ctx context.Context, wilayaCode int) ([]Commune, error)
	Commune(ctx context.Context, wilayaCode int, name string) (string, error)
	Desks(ctx context.Context, wilayaCode int) ([]Desk, error)
	Desk(ctx context.Context, wilayaCode int, id string) (Desk, error)
	CreateOrder(ctx context.Context, req OrderRequest) (Shipment, error)
	Track(ctx context.Context, courierKey, trackingNumber string) (Tracking, error)
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

// Client resolves Courier calls. It owns the caches and the whole fallback
// decision, and asks one of two adapters for the answer: the dzship HTTP API or
// the bundled delivery-map snapshots.
type Client struct {
	remote       source
	snapshot     source
	localSandbox bool
	onFallback   func(string)
	ttl          time.Duration
	now          func() time.Time

	mu           sync.Mutex
	wilayas      cache[[]Wilaya]
	communes     map[int]*cache[[]Commune]
	desks        map[int]*cache[[]Desk]
	offlineUntil time.Time
}

// source is one adapter behind Client. The methods mirror the delivery map and
// parcel verbs without carrying any cache or policy.
type source interface {
	wilayas(ctx context.Context) ([]Wilaya, error)
	communes(ctx context.Context, wilayaCode int) ([]Commune, error)
	desks(ctx context.Context, wilayaCode int) ([]Desk, error)
	createOrder(ctx context.Context, req OrderRequest) (Shipment, error)
	track(ctx context.Context, courierKey, trackingNumber string) (Tracking, error)
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
		remote:       newDZShip(opts, timeout),
		snapshot:     snapshotSource{now: time.Now},
		localSandbox: localSandbox,
		onFallback:   opts.OnFallback,
		ttl:          ttl,
		now:          time.Now,
		communes:     make(map[int]*cache[[]Commune]),
		desks:        make(map[int]*cache[[]Desk]),
	}
}

func (c *Client) Wilayas(ctx context.Context) ([]Wilaya, error) {
	return resolveMap(ctx, c, &c.wilayas, "wilayas",
		c.remote.wilayas, c.snapshot.wilayas, func(error) bool { return true })
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
	return resolveMap(ctx, c, c.communeCache(wilayaCode), "communes",
		func(ctx context.Context) ([]Commune, error) { return c.remote.communes(ctx, wilayaCode) },
		func(ctx context.Context) ([]Commune, error) { return c.snapshot.communes(ctx, wilayaCode) },
		func(error) bool { return true })
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
	return resolveMap(ctx, c, c.deskCache(wilayaCode), "desks",
		func(ctx context.Context) ([]Desk, error) { return c.remote.desks(ctx, wilayaCode) },
		func(ctx context.Context) ([]Desk, error) { return c.snapshot.desks(ctx, wilayaCode) },
		c.sandboxUnavailable)
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
	shipment, err := resolveCall(ctx, c, "orders",
		func(ctx context.Context) (Shipment, error) { return c.remote.createOrder(ctx, req) },
		func(ctx context.Context) (Shipment, error) { return c.snapshot.createOrder(ctx, req) })
	if err != nil {
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
	return resolveCall(ctx, c, "track",
		func(ctx context.Context) (Tracking, error) { return c.remote.track(ctx, courierKey, trackingNumber) },
		func(ctx context.Context) (Tracking, error) { return c.snapshot.track(ctx, courierKey, trackingNumber) })
}

// resolveMap is the one owner of the delivery-map decision: fresh cache, the
// local sandbox while the courier is offline, one adapter call, the stale
// cache, then the bundled snapshot. localOn says whether this resource has a
// local stand-in when the adapter fails; the map itself always does.
func resolveMap[T any](ctx context.Context, c *Client, entry *cache[T], resource string,
	fetch, local func(context.Context) (T, error), localOn func(error) bool) (T, error) {
	var zero T
	if value, fresh := entry.get(c.now()); fresh {
		return value, nil
	}
	if c.isOffline() {
		if value, err := local(ctx); err == nil {
			c.notifyFallback(resource)
			entry.set(value, c.now().Add(5*time.Minute))
			return value, nil
		}
	}
	value, err := fetch(ctx)
	if err != nil {
		if stale, ok := entry.stale(); ok {
			return stale, nil
		}
		if localOn(err) {
			if fallback, localErr := local(ctx); localErr == nil {
				c.markOffline()
				c.notifyFallback(resource)
				entry.set(fallback, c.now().Add(5*time.Minute))
				return fallback, nil
			}
		}
		return zero, err
	}
	entry.set(value, c.now().Add(c.ttl))
	return value, nil
}

// resolveCall is the shipment counterpart of resolveMap, for the two verbs
// with no cache to hold a stale answer.
func resolveCall[T any](ctx context.Context, c *Client, resource string,
	fetch, local func(context.Context) (T, error)) (T, error) {
	var zero T
	if c.isOffline() {
		if value, err := local(ctx); err == nil {
			c.notifyFallback(resource)
			return value, nil
		}
	}
	value, err := fetch(ctx)
	if err != nil {
		if c.sandboxUnavailable(err) {
			if fallback, localErr := local(ctx); localErr == nil {
				c.markOffline()
				c.notifyFallback(resource)
				return fallback, nil
			}
		}
		return zero, err
	}
	return value, nil
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

func (c *Client) notifyFallback(resource string) {
	if c.onFallback != nil {
		c.onFallback(resource)
	}
}

// sandboxUnavailable keeps a real courier key from ever receiving the local
// sandbox's desks, parcels or tracking.
func (c *Client) sandboxUnavailable(err error) bool {
	return c.localSandbox && isUnavailable(err)
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
