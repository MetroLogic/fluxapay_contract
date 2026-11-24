# FluxaPay Go SDK — Quickstart

Official Go client for the FluxaPay payments API.

## 1. Installation

```bash
go get github.com/MetroLogic/fluxapay-go
```

Then import it in your code:

```go
import fluxapay "github.com/MetroLogic/fluxapay-go"
```

## 2. Authentication

Create a client with your API key. Never hardcode secrets — read them from the environment.

```go
package main

import (
	"log"
	"os"

	fluxapay "github.com/MetroLogic/fluxapay-go"
)

func main() {
	apiKey := os.Getenv("FLUXAPAY_API_KEY")
	if apiKey == "" {
		log.Fatal("FLUXAPAY_API_KEY is not set")
	}

	client := fluxapay.NewClient(fluxapay.Config{
		APIKey:  apiKey,
		BaseURL: fluxapay.TestnetBaseURL, // use fluxapay.MainnetBaseURL in production
	})

	_ = client
}
```

## 3. Create a payment

```go
package main

import (
	"context"
	"fmt"
	"log"
	"os"

	fluxapay "github.com/MetroLogic/fluxapay-go"
)

func main() {
	client := fluxapay.NewClient(fluxapay.Config{
		APIKey:  os.Getenv("FLUXAPAY_API_KEY"),
		BaseURL: fluxapay.TestnetBaseURL,
	})

	payment, err := client.Payments.Create(context.Background(), &fluxapay.CreatePaymentParams{
		Amount:   "25.00",
		Currency: "USD",
		Reference: "order-1234",
	})
	if err != nil {
		log.Fatalf("create payment: %v", err)
	}

	fmt.Printf("payment %s created, status=%s\n", payment.ID, payment.Status)
}
```

## 4. Check payment status (with polling)

```go
package main

import (
	"context"
	"fmt"
	"log"
	"os"
	"time"

	fluxapay "github.com/MetroLogic/fluxapay-go"
)

func main() {
	client := fluxapay.NewClient(fluxapay.Config{
		APIKey:  os.Getenv("FLUXAPAY_API_KEY"),
		BaseURL: fluxapay.TestnetBaseURL,
	})

	paymentID := os.Getenv("FLUXAPAY_PAYMENT_ID")
	if paymentID == "" {
		log.Fatal("FLUXAPAY_PAYMENT_ID is not set")
	}

	ctx := context.Background()
	for i := 0; i < 30; i++ {
		payment, err := client.Payments.Get(ctx, paymentID)
		if err != nil {
			log.Fatalf("get payment: %v", err)
		}

		fmt.Printf("status=%s\n", payment.Status)
		if payment.Status == fluxapay.PaymentStatusSucceeded || payment.Status == fluxapay.PaymentStatusFailed {
			return
		}

		time.Sleep(2 * time.Second)
	}

	log.Fatal("timed out waiting for payment to settle")
}
```

## 5. Create a payment link

```go
package main

import (
	"context"
	"fmt"
	"log"
	"os"

	fluxapay "github.com/MetroLogic/fluxapay-go"
)

func main() {
	client := fluxapay.NewClient(fluxapay.Config{
		APIKey:  os.Getenv("FLUXAPAY_API_KEY"),
		BaseURL: fluxapay.TestnetBaseURL,
	})

	link, err := client.PaymentLinks.Create(context.Background(), &fluxapay.CreatePaymentLinkParams{
		Amount:      "25.00",
		Currency:    "USD",
		Description: "Order #1234",
	})
	if err != nil {
		log.Fatalf("create payment link: %v", err)
	}

	fmt.Printf("share this link: %s\n", link.URL)
}
```

## 6. Verify a webhook

Always verify the signature before trusting a webhook payload.

```go
package main

import (
	"fmt"
	"io"
	"log"
	"net/http"
	"os"

	fluxapay "github.com/MetroLogic/fluxapay-go"
)

func main() {
	secret := os.Getenv("FLUXAPAY_WEBHOOK_SECRET")
	if secret == "" {
		log.Fatal("FLUXAPAY_WEBHOOK_SECRET is not set")
	}

	http.HandleFunc("/webhooks/fluxapay", func(w http.ResponseWriter, r *http.Request) {
		body, err := io.ReadAll(r.Body)
		if err != nil {
			http.Error(w, "bad request", http.StatusBadRequest)
			return
		}

		event, err := fluxapay.Webhooks.ConstructEvent(body, r.Header.Get("Fluxapay-Signature"), secret)
		if err != nil {
			http.Error(w, "invalid signature", http.StatusBadRequest)
			return
		}

		fmt.Printf("received event %s of type %s\n", event.ID, event.Type)
		w.WriteHeader(http.StatusOK)
	})

	log.Fatal(http.ListenAndServe(":8080", nil))
}
```

## 7. Error handling

The SDK returns typed errors so you can branch on the failure kind and retry transient failures.

```go
package main

import (
	"context"
	"errors"
	"fmt"
	"log"
	"os"
	"time"

	fluxapay "github.com/MetroLogic/fluxapay-go"
)

func createWithRetry(client *fluxapay.Client, params *fluxapay.CreatePaymentParams) (*fluxapay.Payment, error) {
	var lastErr error
	for attempt := 0; attempt < 3; attempt++ {
		payment, err := client.Payments.Create(context.Background(), params)
		if err == nil {
			return payment, nil
		}
		lastErr = err

		var apiErr *fluxapay.APIError
		if errors.As(err, &apiErr) {
			// Do not retry client errors (4xx) other than rate limits.
			if apiErr.StatusCode >= 400 && apiErr.StatusCode < 500 && apiErr.StatusCode != 429 {
				return nil, err
			}
		}

		time.Sleep(time.Duration(attempt+1) * time.Second)
	}
	return nil, lastErr
}

func main() {
	client := fluxapay.NewClient(fluxapay.Config{
		APIKey:  os.Getenv("FLUXAPAY_API_KEY"),
		BaseURL: fluxapay.TestnetBaseURL,
	})

	payment, err := createWithRetry(client, &fluxapay.CreatePaymentParams{
		Amount:   "25.00",
		Currency: "USD",
	})
	if err != nil {
		var apiErr *fluxapay.APIError
		if errors.As(err, &apiErr) {
			log.Fatalf("api error %d: %s", apiErr.StatusCode, apiErr.Message)
		}
		log.Fatalf("create payment: %v", err)
	}

	fmt.Printf("payment %s created\n", payment.ID)
}
```

## Runnable examples

See [`examples/`](./examples) for complete programs:

- `examples/create_payment` — create a payment
- `examples/check_status` — poll a payment until it settles
- `examples/verify_webhook` — verify and handle a webhook

All examples read credentials from environment variables:

```bash
export FLUXAPAY_API_KEY="your-testnet-key"
go run ./examples/create_payment
```
