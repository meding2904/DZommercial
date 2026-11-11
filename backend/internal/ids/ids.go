package ids

import (
	"crypto/rand"
	"encoding/hex"
)

// New returns a prefixed random identifier, for example prd_9f2c....
func New(prefix string) string {
	buf := make([]byte, 12)
	if _, err := rand.Read(buf); err != nil {
		panic("crypto/rand unavailable: " + err.Error())
	}
	return prefix + "_" + hex.EncodeToString(buf)
}
