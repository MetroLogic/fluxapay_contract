package fluxapay

import (
	"context"
	"fmt"
	"net/http"
	"net/url"
	"time"
)

// CreatePaymentLink creates a new payment link via POST /payment-links.
func (c *Client) CreatePaymentLink(ctx context.Context, args CreatePaymentLinkArgs) (*PaymentLink, error) {
	if args.Amount <= 0 {
		return nil, fmt.Errorf("fluxapay: amount must be greater than zero")
	}
	if args.Currency == "" {
		return nil, fmt.Errorf("fluxapay: currency is required")
	}

	var link PaymentLink
	if err := c.do(ctx, http.MethodPost, "/payment-links", args, &link); err != nil {
		return nil, err
	}
	return &link, nil
}

// GetPaymentLink fetches a payment link by ID via GET /payment-links/{id}.
func (c *Client) GetPaymentLink(ctx context.Context, id string) (*PaymentLink, error) {
	if id == "" {
		return nil, fmt.Errorf("fluxapay: payment link id is required")
	}

	var link PaymentLink
	path := "/payment-links/" + url.PathEscape(id)
	if err := c.do(ctx, http.MethodGet, path, nil, &link); err != nil {
		return nil, err
	}
	return &link, nil
}

// PollPaymentStatus polls GetPaymentLink until the link status is Confirmed
// or the timeout elapses. It returns the last observed link on timeout along
// with a non-nil error.
func (c *Client) PollPaymentStatus(ctx context.Context, id string, timeout time.Duration) (*PaymentLink, error) {
	if id == "" {
		return nil, fmt.Errorf("fluxapay: payment link id is required")
	}

	deadline := time.Now().Add(timeout)
	interval := c.pollInterval
	if interval <= 0 {
		interval = 2 * time.Second
	}

	var last *PaymentLink
	for {
		link, err := c.GetPaymentLink(ctx, id)
		if err != nil {
			return last, err
		}
		last = link

		if link.Status == StatusConfirmed {
			return link, nil
		}

		if !time.Now().Add(interval).Before(deadline) {
			return last, fmt.Errorf("fluxapay: timed out waiting for payment link %s to be confirmed (last status: %s)", id, link.Status)
		}

		select {
		case <-ctx.Done():
			return last, ctx.Err()
		case <-time.After(interval):
		}
	}
}
