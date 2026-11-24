"""Unit tests for the Python SDK payment methods.

All HTTP calls are intercepted with respx; no real network traffic occurs.
"""

import hashlib
import hmac
import json

import httpx
import pytest
import respx

from handsdk import HandsClient
from handsdk.exceptions import (
    AuthenticationError,
    NotFoundError,
    ValidationError,
)

BASE_URL = "https://api.handstoken.com"


@pytest.fixture
def client():
    return HandsClient(api_key="test_api_key", base_url=BASE_URL)


@pytest.fixture
def payment_payload():
    return {
        "amount": "100.00",
        "currency": "USD",
        "reference": "order-123",
        "callback_url": "https://merchant.example/callback",
    }


@pytest.fixture
def payment_response():
    return {
        "id": "pay_123",
        "status": "pending",
        "amount": "100.00",
        "currency": "USD",
        "reference": "order-123",
        "payment_url": "https://pay.handstoken.com/pay_123",
        "created_at": "2024-01-01T00:00:00Z",
    }


@respx.mock
def test_create_payment_happy_path(client, payment_payload, payment_response):
    route = respx.post(f"{BASE_URL}/v1/payments").mock(
        return_value=httpx.Response(201, json=payment_response)
    )

    payment = client.create_payment(**payment_payload)

    assert route.called
    assert payment.id == "pay_123"
    assert payment.status == "pending"
    assert payment.amount == "100.00"
    assert payment.payment_url == "https://pay.handstoken.com/pay_123"


@respx.mock
def test_create_payment_validation_error(client, payment_payload):
    respx.post(f"{BASE_URL}/v1/payments").mock(
        return_value=httpx.Response(
            400,
            json={"error": "invalid_amount", "message": "Amount must be positive"},
        )
    )

    with pytest.raises(ValidationError) as exc_info:
        client.create_payment(**payment_payload)

    assert "Amount must be positive" in str(exc_info.value)


@respx.mock
def test_create_payment_auth_error(client, payment_payload):
    respx.post(f"{BASE_URL}/v1/payments").mock(
        return_value=httpx.Response(
            401, json={"error": "unauthorized", "message": "Invalid API key"}
        )
    )

    with pytest.raises(AuthenticationError):
        client.create_payment(**payment_payload)


@respx.mock
def test_get_payment_found(client, payment_response):
    route = respx.get(f"{BASE_URL}/v1/payments/pay_123").mock(
        return_value=httpx.Response(200, json=payment_response)
    )

    payment = client.get_payment("pay_123")

    assert route.called
    assert payment.id == "pay_123"


@respx.mock
def test_get_payment_not_found(client):
    respx.get(f"{BASE_URL}/v1/payments/pay_missing").mock(
        return_value=httpx.Response(
            404, json={"error": "not_found", "message": "Payment not found"}
        )
    )

    with pytest.raises(NotFoundError):
        client.get_payment("pay_missing")


@respx.mock
def test_verify_payment_success(client):
    respx.get(f"{BASE_URL}/v1/payments/pay_123/verify").mock(
        return_value=httpx.Response(
            200, json={"verified": True, "status": "completed"}
        )
    )

    result = client.verify_payment("pay_123")

    assert result.verified is True
    assert result.status == "completed"


@respx.mock
def test_verify_payment_hash_mismatch(client):
    respx.get(f"{BASE_URL}/v1/payments/pay_123/verify").mock(
        return_value=httpx.Response(
            422,
            json={"error": "hash_mismatch", "message": "Payment hash mismatch"},
        )
    )

    with pytest.raises(ValidationError) as exc_info:
        client.verify_payment("pay_123")

    assert "hash mismatch" in str(exc_info.value).lower()


@respx.mock
def test_create_refund_success(client):
    refund_response = {
        "id": "ref_123",
        "payment_id": "pay_123",
        "amount": "25.00",
        "status": "pending",
        "created_at": "2024-01-02T00:00:00Z",
    }
    route = respx.post(f"{BASE_URL}/v1/payments/pay_123/refunds").mock(
        return_value=httpx.Response(201, json=refund_response)
    )

    refund = client.create_refund("pay_123", amount="25.00")

    assert route.called
    assert refund.id == "ref_123"
    assert refund.amount == "25.00"


@respx.mock
def test_create_refund_exceeds_payment_amount(client):
    respx.post(f"{BASE_URL}/v1/payments/pay_123/refunds").mock(
        return_value=httpx.Response(
            422,
            json={
                "error": "amount_exceeds_payment",
                "message": "Refund amount exceeds payment amount",
            },
        )
    )

    with pytest.raises(ValidationError) as exc_info:
        client.create_refund("pay_123", amount="500.00")

    assert "exceeds" in str(exc_info.value).lower()


@respx.mock
def test_create_payment_missing_required_field(client):
    respx.post(f"{BASE_URL}/v1/payments").mock(
        return_value=httpx.Response(
            200,
            json={
                "id": "pay_123",
                "status": "pending",
                "currency": "USD",
            },
        )
    )

    with pytest.raises(Exception):
        client.create_payment(
            amount="100.00",
            currency="USD",
            reference="order-123",
            callback_url="https://merchant.example/callback",
        )


@respx.mock
def test_create_payment_wrong_field_type(client):
    respx.post(f"{BASE_URL}/v1/payments").mock(
        return_value=httpx.Response(
            200,
            json={
                "id": 12345,
                "status": "pending",
                "amount": "100.00",
                "currency": "USD",
                "reference": "order-123",
                "payment_url": "https://pay.handstoken.com/pay_123",
                "created_at": "2024-01-01T00:00:00Z",
            },
        )
    )

    with pytest.raises(Exception):
        client.create_payment(
            amount="100.00",
            currency="USD",
            reference="order-123",
            callback_url="https://merchant.example/callback",
        )
