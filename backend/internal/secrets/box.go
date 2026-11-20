// Package secrets encrypts small settings values before they are stored in the
// database. The key comes from PLUGIN_SECRET_KEY; it is hashed to 32 bytes so any
// sufficiently long operator passphrase works.
package secrets

import (
	"crypto/aes"
	"crypto/cipher"
	"crypto/rand"
	"crypto/sha256"
	"encoding/base64"
	"errors"
	"fmt"
	"strings"
)

const prefix = "enc:v1:"

var ErrInvalid = errors.New("the encrypted value is not valid")

type Box struct {
	key [32]byte
}

func New(key string) Box {
	return Box{key: sha256.Sum256([]byte(key))}
}

func (b Box) Encrypt(plain []byte) (string, error) {
	block, err := aes.NewCipher(b.key[:])
	if err != nil {
		return "", fmt.Errorf("cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return "", fmt.Errorf("gcm: %w", err)
	}
	nonce := make([]byte, gcm.NonceSize())
	if _, err := rand.Read(nonce); err != nil {
		return "", fmt.Errorf("nonce: %w", err)
	}
	sealed := gcm.Seal(nonce, nonce, plain, nil)
	return prefix + base64.RawURLEncoding.EncodeToString(sealed), nil
}

func (b Box) Decrypt(value string) ([]byte, error) {
	if !strings.HasPrefix(value, prefix) {
		return nil, ErrInvalid
	}
	raw, err := base64.RawURLEncoding.DecodeString(strings.TrimPrefix(value, prefix))
	if err != nil {
		return nil, ErrInvalid
	}
	block, err := aes.NewCipher(b.key[:])
	if err != nil {
		return nil, fmt.Errorf("cipher: %w", err)
	}
	gcm, err := cipher.NewGCM(block)
	if err != nil {
		return nil, fmt.Errorf("gcm: %w", err)
	}
	if len(raw) < gcm.NonceSize() {
		return nil, ErrInvalid
	}
	plain, err := gcm.Open(nil, raw[:gcm.NonceSize()], raw[gcm.NonceSize():], nil)
	if err != nil {
		return nil, ErrInvalid
	}
	return plain, nil
}
