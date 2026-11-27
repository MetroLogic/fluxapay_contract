"""Duplicate-delivery test for python_receiver.py.

    python3 -m unittest test_python_receiver -v
"""

import hashlib
import hmac
import json
import os
import sqlite3
import sys
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
os.environ.setdefault("FLUXAPAY_WEBHOOK_SECRET", "whsec_test_secret")
os.environ.setdefault("FLUXAPAY_DB", ":memory:")

import python_receiver  # noqa: E402

SECRET = b"whsec_test_secret"
NOW = 1_767_225_600
PAYMENT = ["pay_1", "1000000", "USDC", "confirmed"]


def envelope(**overrides):
    event = {
        "id": "whd_00000000-0000-0000-0000-000000000001",
        "event_id": "evt_test_payment_confirmed",
        "type": "payment.confirmed",
        "createdAt": "2026-01-01T00:00:00.000Z",
        "livemode": True,
        "data": {
            "payment_id": PAYMENT[0],
            "merchant_id": "GA7NQQNLFQC7OQF6MERCHANT",
            "amount": PAYMENT[1],
            "currency": PAYMENT[2],
            "status": PAYMENT[3],
        },
    }
    event.update(overrides)
    return event


def post(receiver, event, timestamp=NOW, raw=None):
    body = raw if raw is not None else json.dumps(event).encode()
    signature = hmac.new(
        SECRET, f"{timestamp}.".encode() + body, hashlib.sha256
    ).hexdigest()
    return receiver.handle(
        body,
        {
            "X-FluxaPay-Signature": f"sha256={signature}",
            "X-FluxaPay-Timestamp": str(timestamp),
            "X-FluxaPay-Event": event.get("type", ""),
        },
        now_seconds=NOW,
    )


class DuplicateDeliveryTest(unittest.TestCase):
    def setUp(self):
        self.conn = python_receiver.connect(":memory:")
        self.conn.execute("CREATE TABLE applied_side_effects (event_id TEXT NOT NULL)")
        self.receiver = python_receiver.WebhookReceiver(
            self.conn, SECRET, self._apply
        )

    def tearDown(self):
        self.conn.close()

    @staticmethod
    def _apply(conn, event):
        conn.execute(
            "INSERT INTO applied_side_effects (event_id) VALUES (?)",
            (event["event_id"],),
        )

    def applied(self):
        return self.conn.execute(
            "SELECT COUNT(*) FROM applied_side_effects"
        ).fetchone()[0]

    def recorded(self):
        return self.conn.execute(
            "SELECT COUNT(*) FROM processed_webhook_events"
        ).fetchone()[0]

    def test_duplicate_delivery_applied_once(self):
        first = post(self.receiver, envelope())
        self.assertEqual(first, (200, {"received": True, "duplicate": False}))

        second = post(self.receiver, envelope())
        self.assertEqual(second, (200, {"received": True, "duplicate": True}))

        self.assertEqual(self.applied(), 1)
        self.assertEqual(self.recorded(), 1)

    def test_distinct_events_for_same_payment_both_apply(self):
        post(self.receiver, envelope())
        post(
            self.receiver,
            envelope(
                event_id="evt_test_payment_created",
                type="payment.created",
                data={**envelope()["data"], "status": "created"},
            ),
        )
        self.assertEqual(self.applied(), 2)

    def test_refund_dedups_on_event_id(self):
        refund = envelope(
            event_id="evt_test_refund_completed",
            type="refund.completed",
            data={
                "refund_id": "ref_1",
                "payment_id": "pay_1",
                "amount": "500000",
                "status": "completed",
            },
        )
        post(self.receiver, refund)
        status, body = post(self.receiver, refund)
        self.assertEqual(status, 200)
        self.assertTrue(body["duplicate"])
        self.assertEqual(self.applied(), 1)

    def test_rejected_signature_applies_nothing(self):
        body = json.dumps(envelope()).encode()
        status, _ = self.receiver.handle(
            body,
            {
                "X-FluxaPay-Signature": "sha256=" + "0" * 64,
                "X-FluxaPay-Timestamp": str(NOW),
            },
            now_seconds=NOW,
        )
        self.assertEqual(status, 401)
        self.assertEqual(self.applied(), 0)

    def test_stale_timestamp_is_rejected(self):
        status, _ = post(self.receiver, envelope(), timestamp=NOW - 301)
        self.assertEqual(status, 401)
        self.assertEqual(self.applied(), 0)

    def test_crash_rolls_back_claim_and_retry_succeeds(self):
        state = {"fail": True}
        original = self.receiver.apply_event

        def flaky(conn, event):
            if state["fail"]:
                state["fail"] = False
                raise RuntimeError("simulated crash")
            original(conn, event)

        self.receiver.apply_event = flaky

        with self.assertRaises(RuntimeError):
            post(self.receiver, envelope())
        self.assertEqual(self.applied(), 0)
        self.assertEqual(self.recorded(), 0, "claim must not survive rollback")

        status, body = post(self.receiver, envelope())
        self.assertEqual(status, 200)
        self.assertFalse(body["duplicate"])
        self.assertEqual(self.applied(), 1)


if __name__ == "__main__":
    unittest.main()
