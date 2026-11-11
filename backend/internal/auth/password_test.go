package auth

import (
	"strings"
	"testing"
)

func TestHashAndVerifyPassword(t *testing.T) {
	hash, err := HashPassword("a long enough password")
	if err != nil {
		t.Fatalf("HashPassword: %v", err)
	}
	if !strings.HasPrefix(hash, "$argon2id$v=19$m=65536,t=3,p=4$") {
		t.Fatalf("unexpected PHC prefix: %s", hash)
	}
	if !VerifyPassword("a long enough password", hash) {
		t.Fatal("correct password was rejected")
	}
	if VerifyPassword("another password", hash) {
		t.Fatal("wrong password was accepted")
	}
	if VerifyPassword("a long enough password", "not-a-hash") {
		t.Fatal("malformed hash was accepted")
	}
}

func TestHashIsSalted(t *testing.T) {
	first, err := HashPassword("same password twice")
	if err != nil {
		t.Fatal(err)
	}
	second, err := HashPassword("same password twice")
	if err != nil {
		t.Fatal(err)
	}
	if first == second {
		t.Fatal("two hashes of the same password should differ")
	}
}

func TestVerifyRejectsRaisedParametersOutsideLimits(t *testing.T) {
	hash := "$argon2id$v=19$m=999999999,t=3,p=4$c2FsdHNhbHRzYWx0$aGFzaGhhc2hoYXNoaGFzaA"
	if VerifyPassword("password", hash) {
		t.Fatal("hash with an unreasonable memory cost was accepted")
	}
}
