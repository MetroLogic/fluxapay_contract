// Command create_payment creates a payment against the FluxaPay testnet.
//
// Usage:
//
//	export FLUXAPAY_API_KEY="your-testnet-key"
//	go run ./examples/create_payment
package main

import (
	"context"
	"fmt"
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
		BaseURL: fluxapay.TestnetBaseURL,
	})

	payment, err := client.Payments.Create(context.Background(), &fluxapay.CreatePaymentParams{
		Amount:    "25.00",
		Currency:  "USD",
		Reference: "order-1234",
	})
	if err != nil {
		log.Fatalf("create payment: %v", err)
	}

	fmt.Printf("payment %s created, status=%s\n", payment.ID, payment.Status)
}
