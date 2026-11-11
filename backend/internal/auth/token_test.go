package auth

import (
	"encoding/base64"
	"regexp"
	"testing"
)

func TestNewToken(t *testing.T) {
	first, err := NewToken()
	if err != nil {
		t.Fatal(err)
	}
	second, err := NewToken()
	if err != nil {
		t.Fatal(err)
	}
	if first == second {
		t.Fatal("tokens must be unique")
	}
	raw, err := base64.RawURLEncoding.DecodeString(first)
	if err != nil {
		t.Fatalf("token is not base64url: %v", err)
	}
	if len(raw) != TokenBytes {
		t.Fatalf("token has %d bytes, want %d", len(raw), TokenBytes)
	}
	if matched, _ := regexp.MatchString(`^[A-Za-z0-9_-]{43}$`, first); !matched {
		t.Fatalf("unexpected token format: %s", first)
	}
}

func TestHashToken(t *testing.T) {
	hash := HashToken("token-value")
	if len(hash) != 64 {
		t.Fatalf("hash length %d, want 64", len(hash))
	}
	if matched, _ := regexp.MatchString(`^[0-9a-f]{64}$`, hash); !matched {
		t.Fatalf("hash is not lowercase hex: %s", hash)
	}
	if HashToken("token-value") != hash {
		t.Fatal("hash is not deterministic")
	}
	if HashToken("token-other") == hash {
		t.Fatal("different tokens produced the same hash")
	}
}
