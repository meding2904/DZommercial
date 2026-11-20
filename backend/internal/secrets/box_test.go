package secrets_test

import (
	"testing"

	"azahar/internal/secrets"
)

func TestRoundTrip(t *testing.T) {
	box := secrets.New("a long development key for tests")
	sealed, err := box.Encrypt([]byte(`{"api_key":"sk-test"}`))
	if err != nil {
		t.Fatalf("encrypt: %v", err)
	}
	if string(sealed) == `{"api_key":"sk-test"}` {
		t.Fatal("the plaintext survived encryption")
	}
	plain, err := box.Decrypt(sealed)
	if err != nil {
		t.Fatalf("decrypt: %v", err)
	}
	if string(plain) != `{"api_key":"sk-test"}` {
		t.Fatalf("unexpected plaintext: %s", plain)
	}
}

func TestWrongKeyFails(t *testing.T) {
	sealed, err := secrets.New("key one").Encrypt([]byte("value"))
	if err != nil {
		t.Fatalf("encrypt: %v", err)
	}
	if _, err := secrets.New("key two").Decrypt(sealed); err == nil {
		t.Fatal("a different key must not decrypt the value")
	}
}

func TestRejectsPlaintext(t *testing.T) {
	if _, err := secrets.New("key").Decrypt(`{"api_key":"sk-test"}`); err == nil {
		t.Fatal("plaintext must not pass as encrypted data")
	}
}

func TestTamperFails(t *testing.T) {
	box := secrets.New("key")
	sealed, err := box.Encrypt([]byte("value"))
	if err != nil {
		t.Fatalf("encrypt: %v", err)
	}
	tampered := sealed[:len(sealed)-1] + "A"
	if _, err := box.Decrypt(tampered); err == nil {
		t.Fatal("a tampered value must not decrypt")
	}
}
