package api

import (
	"encoding/json"
	"net/http"
)

func (c *Client) ensureOrigin() *Error {
	c.mu.RLock()
	if c.settings.Origin != "" || c.resolvedOrigin != "" {
		c.mu.RUnlock()
		return nil
	}
	generation := c.generation
	host, hostErr := apiHost(c.settings.APIURL)
	c.mu.RUnlock()
	if hostErr != nil {
		return hostErr
	}

	c.Health()

	c.mu.Lock()
	defer c.mu.Unlock()
	if c.generation != generation {
		return settingsChangedError()
	}
	if c.settings.Origin != "" || c.resolvedOrigin != "" {
		return nil
	}
	if isLoopbackHost(host) {
		c.resolvedOrigin = fallbackOrigin
		return nil
	}
	return newError(0, "origin_unavailable",
		"Enter the request origin in Connection before connecting to this server.")
}

func (c *Client) Health() Health {
	target, generation, err := c.targetURL("/api/health")
	if err != nil {
		return Health{OK: false, Status: 0, Message: err.Message}
	}

	result, requestErr := c.execute(requestOptions{
		method:        http.MethodGet,
		target:        target,
		generation:    generation,
		timeout:       c.timeout,
		includeOrigin: false,
		includeCSRF:   false,
		maxBytes:      maxResponseBytes,
	})
	if requestErr != nil {
		return Health{OK: false, Status: 0, Message: requestErr.Message}
	}
	if result.status < 200 || result.status > 299 {
		return Health{
			OK:      false,
			Status:  result.status,
			Message: "The API answered, but not with a healthy status.",
		}
	}

	var body struct {
		Origin     string `json:"origin"`
		Env        string `json:"env"`
		ServerTime string `json:"server_time"`
	}
	_ = json.Unmarshal(result.body, &body)

	if origin, originErr := normalizeOrigin(body.Origin); originErr == nil && origin != "" {
		c.mu.Lock()
		if c.generation == generation {
			c.resolvedOrigin = origin
		}
		c.mu.Unlock()
	}

	c.mu.RLock()
	origin := c.settings.Origin
	if origin == "" {
		origin = c.resolvedOrigin
	}
	c.mu.RUnlock()

	return Health{
		OK:         true,
		Status:     result.status,
		Env:        body.Env,
		Origin:     origin,
		ServerTime: body.ServerTime,
	}
}
