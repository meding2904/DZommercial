package httpapi

import (
	"bytes"
	"crypto/rand"
	"crypto/subtle"
	"encoding/base64"
	"errors"
	"fmt"
	"image"
	"image/color"
	"image/png"
	"math/big"
	mathrand "math/rand"
	"net/http"
	"strings"
	"sync"
	"time"
)

const (
	captchaCodeLength = 5
	captchaAlphabet   = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789"
	captchaMinAge     = 2 * time.Second
	captchaMaxEntries = 5000
)

var errCaptchaBusy = errors.New("captcha store is full")

type captchaEntry struct {
	code    string
	ip      string
	issued  time.Time
	expires time.Time
}

type captchaStore struct {
	mu      sync.Mutex
	entries map[string]captchaEntry
	ttl     time.Duration
	stop    chan struct{}
	once    sync.Once
}

func newCaptchaStore(ttl time.Duration) *captchaStore {
	store := &captchaStore{
		entries: make(map[string]captchaEntry),
		ttl:     ttl,
		stop:    make(chan struct{}),
	}
	go store.cleanupLoop()
	return store
}

func (c *captchaStore) issue(ip string, now time.Time) (string, string, time.Duration, error) {
	code, err := randomCaptchaCode(captchaCodeLength)
	if err != nil {
		return "", "", 0, err
	}
	token, err := randomCaptchaToken()
	if err != nil {
		return "", "", 0, err
	}
	image := captchaImage(code)
	if image == "" {
		return "", "", 0, errors.New("encode captcha image")
	}

	c.mu.Lock()
	defer c.mu.Unlock()

	c.sweepLocked(now)
	if len(c.entries) >= captchaMaxEntries {
		return "", "", 0, errCaptchaBusy
	}

	entry := captchaEntry{
		code:    code,
		ip:      ip,
		issued:  now,
		expires: now.Add(c.ttl),
	}
	c.entries[token] = entry
	return token, image, c.ttl, nil
}

func (c *captchaStore) verify(token, answer, ip string, now time.Time) string {
	token = strings.TrimSpace(token)
	answer = strings.ToUpper(strings.TrimSpace(answer))
	if token == "" || len(token) > 128 {
		return "missing"
	}

	c.mu.Lock()
	defer c.mu.Unlock()

	entry, ok := c.entries[token]
	if !ok {
		return "expired"
	}
	if now.After(entry.expires) {
		delete(c.entries, token)
		return "expired"
	}
	if entry.ip != "" && ip != "" && entry.ip != ip {
		delete(c.entries, token)
		return "expired"
	}
	if now.Sub(entry.issued) < captchaMinAge {
		return "fast"
	}
	if subtle.ConstantTimeCompare([]byte(entry.code), []byte(answer)) != 1 {
		return "wrong"
	}

	delete(c.entries, token)
	return ""
}

func (c *captchaStore) cleanupLoop() {
	ticker := time.NewTicker(5 * time.Minute)
	defer ticker.Stop()
	for {
		select {
		case <-c.stop:
			return
		case now := <-ticker.C:
			c.mu.Lock()
			c.sweepLocked(now)
			c.mu.Unlock()
		}
	}
}

func (c *captchaStore) sweepLocked(now time.Time) {
	for token, entry := range c.entries {
		if now.After(entry.expires) {
			delete(c.entries, token)
		}
	}
}

func (c *captchaStore) close() {
	c.once.Do(func() { close(c.stop) })
}

func randomCaptchaCode(length int) (string, error) {
	result := make([]byte, length)
	limit := big.NewInt(int64(len(captchaAlphabet)))
	for index := range result {
		value, err := rand.Int(rand.Reader, limit)
		if err != nil {
			return "", fmt.Errorf("generate captcha code: %w", err)
		}
		result[index] = captchaAlphabet[value.Int64()]
	}
	return string(result), nil
}

func randomCaptchaToken() (string, error) {
	buffer := make([]byte, 24)
	if _, err := rand.Read(buffer); err != nil {
		return "", fmt.Errorf("generate captcha token: %w", err)
	}
	return base64.RawURLEncoding.EncodeToString(buffer), nil
}

func captchaImage(code string) string {
	const width, height, scale = 220, 64, 4
	img := image.NewRGBA(image.Rect(0, 0, width, height))
	background := color.RGBA{R: 244, G: 236, B: 229, A: 255}
	ink := color.RGBA{R: 44, G: 35, B: 37, A: 255}
	noise := []color.RGBA{
		{R: 216, G: 180, B: 184, A: 255},
		{R: 185, G: 196, B: 184, A: 255},
		{R: 216, G: 194, B: 168, A: 255},
		{R: 201, G: 189, B: 187, A: 255},
	}

	for y := 0; y < height; y++ {
		for x := 0; x < width; x++ {
			img.Set(x, y, background)
		}
	}

	for index := 0; index < 6; index++ {
		drawCaptchaLine(img,
			mathrand.Intn(width), mathrand.Intn(height),
			mathrand.Intn(width), mathrand.Intn(height),
			noise[mathrand.Intn(len(noise))])
	}
	for index := 0; index < 70; index++ {
		img.Set(mathrand.Intn(width), mathrand.Intn(height), noise[mathrand.Intn(len(noise))])
	}

	x := 24
	for _, character := range code {
		glyph, ok := captchaFont[character]
		if !ok {
			continue
		}
		x += 2 + mathrand.Intn(4)
		y := 18 + mathrand.Intn(5) - 2
		drawCaptchaGlyph(img, glyph, x, y, scale, ink)
		x += 5*scale + 8
	}

	for index := 0; index < 3; index++ {
		drawCaptchaLine(img,
			mathrand.Intn(width), mathrand.Intn(height),
			mathrand.Intn(width), mathrand.Intn(height),
			noise[mathrand.Intn(len(noise))])
	}

	var buffer bytes.Buffer
	if err := png.Encode(&buffer, img); err != nil {
		return ""
	}
	return "data:image/png;base64," + base64.StdEncoding.EncodeToString(buffer.Bytes())
}

func drawCaptchaGlyph(img *image.RGBA, glyph [7]string, x, y, scale int, ink color.RGBA) {
	for row, line := range glyph {
		for column, value := range line {
			if value != '1' {
				continue
			}
			for dy := 0; dy < scale; dy++ {
				for dx := 0; dx < scale; dx++ {
					img.Set(x+column*scale+dx, y+row*scale+dy, ink)
				}
			}
		}
	}
}

func drawCaptchaLine(img *image.RGBA, x0, y0, x1, y1 int, ink color.RGBA) {
	dx := absInt(x1 - x0)
	dy := absInt(y1 - y0)
	steps := dx
	if dy > steps {
		steps = dy
	}
	if steps == 0 {
		img.Set(x0, y0, ink)
		return
	}
	for step := 0; step <= steps; step++ {
		x := x0 + (x1-x0)*step/steps
		y := y0 + (y1-y0)*step/steps
		img.Set(x, y, ink)
	}
}

func absInt(value int) int {
	if value < 0 {
		return -value
	}
	return value
}

var captchaFont = map[rune][7]string{
	'A': {"01110", "10001", "10001", "11111", "10001", "10001", "10001"},
	'B': {"11110", "10001", "10001", "11110", "10001", "10001", "11110"},
	'C': {"01111", "10000", "10000", "10000", "10000", "10000", "01111"},
	'D': {"11110", "10001", "10001", "10001", "10001", "10001", "11110"},
	'E': {"11111", "10000", "10000", "11110", "10000", "10000", "11111"},
	'F': {"11111", "10000", "10000", "11110", "10000", "10000", "10000"},
	'G': {"01111", "10000", "10000", "10111", "10001", "10001", "01111"},
	'H': {"10001", "10001", "10001", "11111", "10001", "10001", "10001"},
	'J': {"00111", "00010", "00010", "00010", "10010", "10010", "01100"},
	'K': {"10001", "10010", "10100", "11000", "10100", "10010", "10001"},
	'L': {"10000", "10000", "10000", "10000", "10000", "10000", "11111"},
	'M': {"10001", "11011", "10101", "10101", "10001", "10001", "10001"},
	'N': {"10001", "11001", "10101", "10011", "10001", "10001", "10001"},
	'P': {"11110", "10001", "10001", "11110", "10000", "10000", "10000"},
	'Q': {"01110", "10001", "10001", "10001", "10101", "10010", "01101"},
	'R': {"11110", "10001", "10001", "11110", "10100", "10010", "10001"},
	'S': {"01111", "10000", "10000", "01110", "00001", "00001", "11110"},
	'T': {"11111", "00100", "00100", "00100", "00100", "00100", "00100"},
	'U': {"10001", "10001", "10001", "10001", "10001", "10001", "01110"},
	'V': {"10001", "10001", "10001", "10001", "10001", "01010", "00100"},
	'W': {"10001", "10001", "10001", "10101", "10101", "11011", "10001"},
	'X': {"10001", "10001", "01010", "00100", "01010", "10001", "10001"},
	'Y': {"10001", "10001", "01010", "00100", "00100", "00100", "00100"},
	'Z': {"11111", "00001", "00010", "00100", "01000", "10000", "11111"},
	'2': {"01110", "10001", "00001", "00010", "00100", "01000", "11111"},
	'3': {"11110", "00001", "00001", "01110", "00001", "00001", "11110"},
	'4': {"00010", "00110", "01010", "10010", "11111", "00010", "00010"},
	'5': {"11111", "10000", "10000", "11110", "00001", "00001", "11110"},
	'6': {"01110", "10000", "10000", "11110", "10001", "10001", "01110"},
	'7': {"11111", "00001", "00010", "00100", "01000", "01000", "01000"},
	'8': {"01110", "10001", "10001", "01110", "10001", "10001", "01110"},
	'9': {"01110", "10001", "10001", "01111", "00001", "00001", "01110"},
}

func captchaErrorMessage(reason string) string {
	switch reason {
	case "fast":
		return "Wait a moment, then enter the code."
	case "expired", "missing":
		return "The code expired or was already used. Request a new one."
	default:
		return "The code is wrong. Check the characters and try again."
	}
}

func (s *Server) handleCheckoutChallenge(w http.ResponseWriter, r *http.Request) {
	token, image, ttl, err := s.captcha.issue(clientIP(r, s.cfg.TrustProxy), time.Now())
	if err != nil {
		if errors.Is(err, errCaptchaBusy) {
			writeError(w, http.StatusTooManyRequests, "captcha_busy",
				"The security check is busy. Try again in a moment.")
			return
		}
		writeError(w, http.StatusInternalServerError, "internal_error", "Something went wrong.")
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"token":      token,
		"image":      image,
		"expires_in": int(ttl.Seconds()),
	})
}
