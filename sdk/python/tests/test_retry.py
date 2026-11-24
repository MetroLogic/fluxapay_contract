"""Tests for SDK auto-retry behavior on transient HTTP errors.

All HTTP calls are intercepted with respx; no real network traffic occurs.
"""

import httpx
import pytest
import respx

from soroscan import Client
from soroscan.exceptions import APIError

BASE_URL = "https://api.soroscan.io"


@pytest.fixture
def client():
    return Client(api_key="test_key", base_url=BASE_URL, max_retries=3)


@respx.mock
@pytest.mark.asyncio
async def test_retry_on_429_with_retry_after(client):
    route = respx.post(f"{BASE_URL}/v1/payments").mock(
        side_effect=[
            httpx.Response(429, headers={"Retry-After": "0"}),
            httpx.Response(201, json={"id": "pay_1", "status": "pending"}),
        ]
    )

    payment = await client.create_payment(
        amount=1000, currency="USD", recipient="GABC"
    )

    assert payment.id == "pay_1"
    assert route.call_count == 2


@respx.mock
@pytest.mark.asyncio
async def test_retry_on_503_with_backoff(client):
    route = respx.get(f"{BASE_URL}/v1/payments/pay_1").mock(
        side_effect=[
            httpx.Response(503),
            httpx.Response(503),
            httpx.Response(200, json={"id": "pay_1", "status": "confirmed"}),
        ]
    )

    payment = await client.get_payment("pay_1")

    assert payment.status == "confirmed"
    assert route.call_count == 3


@respx.mock
@pytest.mark.asyncio
async def test_retry_exhausted_raises(client):
    route = respx.get(f"{BASE_URL}/v1/payments/pay_1").mock(
        return_value=httpx.Response(503)
    )

    with pytest.raises(APIError):
        await client.get_payment("pay_1")

    assert route.call_count == client.max_retries + 1


@respx.mock
@pytest.mark.asyncio
async def test_400_not_retried(client):
    route = respx.post(f"{BASE_URL}/v1/payments").mock(
        return_value=httpx.Response(400, json={"error": "invalid amount"})
    )

    with pytest.raises(APIError):
        await client.create_payment(amount=-1, currency="USD", recipient="GABC")

    assert route.call_count == 1
