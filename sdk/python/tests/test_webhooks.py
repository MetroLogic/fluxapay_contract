"""Unit tests for webhook signature verification in the Python SDK.

All HTTP calls are intercepted with respx; no real network traffic occurs.
"""

import hashlib
import hmac
import json
import time

import pytest
import respx

from sdk.python import client as sdk_client


WEBHOOK_SECRET = "whsec_test_secret"


def _sign(payload: bytes, secret: str = WEBHOOK_SECRET, timestamp: int | None = None) -> str:
    ts = int(time.time()) if timestamp is None else timestamp
    signed = f"{ts}.".encode() + payload
    digest = hmac.new(secret.encode(), signed, hashlib.sha256).hexdigest()
    return f"t={ts},v1={digest}"


def test_verify_webhook_signature_valid():
    payload = json.dumps({"event": "payment.succeeded", "id": "pay_123"}).encode()
    signature = _sign(payload)

    assert sdk_client.verify_webhook_signature(
        payload=payload, signature=signature, secret=WEBHOOK_SECRET
    ) is True


def test_verify_webhook_signature_invalid():
    payload = json.dumps({"event": "payment.succeeded", "id": "pay_123"}).encode()
    signature = _sign(payload)
    tampered = payload.replace(b"pay_123", b"pay_999")

    assert sdk_client.verify_webhook_signature(
        payload=tampered, signature=signature, secret=WEBHOOK_SECRET
    ) is False


def test_verify_webhook_signature_wrong_secret():
    payload = b'{"event":"payment.succeeded"}'
    signature = _sign(payload, secret="whsec_other")

    assert sdk_client.verify_webhook_signature(
        payload=payload, signature=signature, secret=WEBHOOK_SECRET
    ) is False


def test_verify_webhook_signature_expired_timestamp():
    payload = b'{"event":"payment.succeeded"}'
    stale_ts = int(time.time()) - 3600
    signature = _sign(payload, timestamp=stale_ts)

    assert sdk_client.verify_webhook_signature(
        payload=payload, signature=signature, secret=WEBHOOK_SECRET
    ) is False


def test_verify_webhook_signature_malformed_header():
    payload = b'{"event":"payment.succeeded"}'

    assert sdk_client.verify_webhook_signature(
        payload=payload, signature="not-a-valid-header", secret=WEBHOOK_SECRET
    ) is False


@respx.mock
@pytest.mark.asyncio
async def test_webhook_endpoint_roundtrip_no_network():
    """Sanity check that webhook-related SDK calls stay fully mocked."""
    route = respx.get("https://api.handsoff.dev/v1/webhooks/wh_123").respond(
        200, json={"id": "wh_123", "url": "https://example.com/hook", "active": True}
    )

    result = await sdk_client.get_webhook("wh_123")

    assert route.called
    assert result["id"] == "wh_123"
