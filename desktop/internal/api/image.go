package api

import (
	"encoding/base64"
	"mime"
	"net/http"
	"strings"
)

const (
	maxImageBytes          = 8 << 20
	imageCacheSize         = 40
	defaultImageCacheBytes = 64 << 20
)

var allowedImageTypes = map[string]bool{
	"image/png":  true,
	"image/jpeg": true,
	"image/webp": true,
	"image/gif":  true,
	"image/avif": true,
}

func (c *Client) Image(path string) Response {
	data, imageErr := c.image(path)
	if imageErr != nil {
		return Response{OK: false, Error: imageErr}
	}
	return Response{OK: true, Data: data}
}

func (c *Client) image(path string) (string, *Error) {
	if !strings.HasPrefix(path, "/api/images/") {
		return "", invalidImage()
	}

	c.mu.RLock()
	cached, ok := c.imageCache[path]
	c.mu.RUnlock()
	if ok {
		return cached, nil
	}

	target, generation, err := c.targetURL(path)
	if err != nil {
		return "", err
	}
	result, requestErr := c.execute(requestOptions{
		method:        http.MethodGet,
		target:        target,
		generation:    generation,
		timeout:       c.imageTimeout,
		includeOrigin: true,
		includeCSRF:   true,
		maxBytes:      maxImageBytes,
	})
	if requestErr != nil {
		if requestErr.Code == "response_too_large" {
			return "", newError(0, "image_failed", "The image is too large.")
		}
		return "", requestErr
	}
	if result.status < 200 || result.status > 299 {
		return "", newError(result.status, "image_failed", "The image could not be loaded.")
	}

	mediaType, _, parseErr := mime.ParseMediaType(result.contentType)
	if parseErr != nil || !allowedImageTypes[mediaType] {
		return "", newError(0, "image_failed", "The API returned an unsupported image.")
	}

	value := "data:" + mediaType + ";base64," + base64.StdEncoding.EncodeToString(result.body)
	c.storeImage(path, value, generation)
	return value, nil
}

func (c *Client) storeImage(key, value string, generation uint64) {
	size := int64(len(value))

	c.mu.Lock()
	defer c.mu.Unlock()

	limit := c.imageCacheByteLimit
	if limit <= 0 {
		limit = defaultImageCacheBytes
	}
	if c.generation != generation || size > limit {
		return
	}
	if c.imageCache == nil {
		c.imageCache = make(map[string]string)
	}
	if old, exists := c.imageCache[key]; exists {
		c.imageCacheBytes -= int64(len(old))
	} else {
		c.imageOrder = append(c.imageOrder, key)
	}
	c.imageCache[key] = value
	c.imageCacheBytes += size

	for (len(c.imageCache) > imageCacheSize || c.imageCacheBytes > limit) && len(c.imageOrder) > 0 {
		oldest := c.imageOrder[0]
		c.imageOrder = c.imageOrder[1:]
		c.imageCacheBytes -= int64(len(c.imageCache[oldest]))
		delete(c.imageCache, oldest)
	}
}

func invalidImage() *Error {
	return newError(0, "invalid_image", "That image is not served by the API.")
}
