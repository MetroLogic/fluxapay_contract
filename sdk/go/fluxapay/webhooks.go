package fluxapay

import (
	"crypto/hmac"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"strings"
)

// ErrInvalidSignature is returned when a webhook signature does not match the
// expected HMAC-SHA256 digest for the given payload.
var ErrInvalidSignature = errors.New("fluxapay: invalid webhook signature")

// VerifyWebhookSignature verifies that the provided signature matches the
// HMAC-SHA256 digest of payload computed with secret. The signature may be
// supplied as a raw hex string or with a "sha256=" prefix, matching the
// format emitted by FluxaPay webhook deliveries.
func VerifyWebhookSignature(payload []byte, signature, secret string) error {
	if secret == "" {
		return errors.New("fluxapay: webhook secret is required")
	}

	sig := strings.TrimSpace(signature)
	sig = strings.TrimPrefix(sig, "sha256=")
	if sig == "" {
		return ErrInvalidSignature
	}

	provided, err := hex.DecodeString(sig)
	if err != nil {
		return ErrInvalidSignature
	}

	mac := hmac.New(sha256.New, []byte(secret))
	mac.Write(payload)
	expected := mac.Sum(nil)

	if !hmac.Equal(provided, expected) {
		return ErrInvalidSignature
	}
	return nil
}
