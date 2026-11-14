package httpapi

import (
	"context"
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strings"
	"time"
)

const turnstileMaxTokenLength = 2048

var turnstileVerifyURL = "https://challenges.cloudflare.com/turnstile/v0/siteverify"

var turnstileHTTPClient = &http.Client{Timeout: 5 * time.Second}

type turnstileResult struct {
	Success    bool     `json:"success"`
	ErrorCodes []string `json:"error-codes"`
}

func verifyTurnstile(ctx context.Context, secret, token, remoteIP string) (bool, error) {
	token = strings.TrimSpace(token)
	if secret == "" || token == "" || len(token) > turnstileMaxTokenLength {
		return false, nil
	}

	form := url.Values{}
	form.Set("secret", secret)
	form.Set("response", token)
	form.Set("remoteip", remoteIP)

	request, err := http.NewRequestWithContext(ctx, http.MethodPost, turnstileVerifyURL,
		strings.NewReader(form.Encode()))
	if err != nil {
		return false, err
	}
	request.Header.Set("Content-Type", "application/x-www-form-urlencoded")

	response, err := turnstileHTTPClient.Do(request)
	if err != nil {
		return false, err
	}
	defer response.Body.Close()

	if response.StatusCode != http.StatusOK {
		return false, fmt.Errorf("turnstile verify: unexpected status %d", response.StatusCode)
	}

	var result turnstileResult
	if err := json.NewDecoder(io.LimitReader(response.Body, 1<<20)).Decode(&result); err != nil {
		return false, err
	}
	return result.Success, nil
}
