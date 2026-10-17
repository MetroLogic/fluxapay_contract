// Command check_status polls a payment until it settles.
//
// Usage:
//
//	export FLUXAPAY_API_KEY="your-testnet-key"
//	export FLUXAPAY_PAYMENT_ID="pay_123"
//	go run ./examples/check_status
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
	apiKey := os.Getenv("FLUXAPAY_API_KEY")
	if apiKey == "" {
		log.Fatal("FLUXAPAY_API_KEY is not set")
	}

	paymentID := os.Getenv("FLUXAPAY_PAYMENT_ID")
	if paymentID == "" {
		log.Fatal("FLUXAPAY_PAYMENT_ID is not set")
	}

	client := fluxapay.NewClient(fluxapay.Config{
		APIKey:  apiKey,
		BaseURL: fluxapay.TestnetBaseURL,
	})

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
