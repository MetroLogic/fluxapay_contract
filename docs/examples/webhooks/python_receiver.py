"""FluxaPay webhook receiver — idempotent reference implementation.

Verifies the HMAC-SHA256 signature, then records the event's stable
``event_id`` and applies the side effect in a single SQLite transaction.
A replayed delivery therefore either sees the committed event (and is
acknowledged as a duplicate) or sees nothing (and is processed fully).

Run:

    pip install fastapi uvicorn
    FLUXAPAY_WEBHOOK_SECRET=whsec_... uvicorn python_receiver:app --port 3000

For Postgres, keep the same ordering and replace the insert with
``INSERT ... ON CONFLICT (event_id) DO NOTHING`` inside one
``BEGIN ... COMMIT`` block.
"""

import hashlib
import hmac
import json
import os
import sqlite3
import time

from fastapi import FastAPI, Header, Request
from fastapi.responses import JSONResponse

TOLERANCE_SECONDS = 300
RETENTION_SECONDS = 7 * 24 * 60 * 60

SCHEMA = """
CREATE TABLE IF NOT EXISTS processed_webhook_events (
    event_id    TEXT PRIMARY KEY,
    event_type  TEXT NOT NULL,
    resource_id TEXT NOT NULL,
    received_at INTEGER NOT NULL
);
"""


def connect(path: str = ":memory:") -> sqlite3.Connection:
    """Open a SQLite connection in autocommit mode for explicit transactions."""
    conn = sqlite3.connect(path, isolation_level=None, check_same_thread=False)
    conn.execute(SCHEMA)
    return conn


def verify_signature(
    secret: bytes,
    signature_header: str | None,
    timestamp_header: str | None,
    raw_body: bytes,
    now_seconds: int | None = None,
) -> bool:
    """Verify ``sha256=<hex>`` over ``<timestamp>.<raw_body>``."""
    if not signature_header or not signature_header.startswith("sha256="):
        return False
    if not timestamp_header:
        return False
    try:
        timestamp = int(timestamp_header)
    except ValueError:
        return False

    now = int(time.time()) if now_seconds is None else now_seconds
    if abs(now - timestamp) > TOLERANCE_SECONDS:
        return False

    expected = hmac.new(
        secret, f"{timestamp}.".encode() + raw_body, hashlib.sha256
    ).hexdigest()
    return hmac.compare_digest(expected, signature_header[len("sha256=") :])


def resource_id_of(data: dict) -> str | None:
    """The payment/refund/dispute the event is about, for correlation only."""
    return data.get("payment_id") or data.get("refund_id") or data.get("dispute_id")


class WebhookReceiver:
    """Signature check + atomic dedup + side effect.

    ``apply_event(conn, event)`` runs inside the claim transaction, so raising
    an exception undoes the claim too and FluxaPay's retry can process the
    event cleanly. It may be a regular function.
    """

    def __init__(self, conn: sqlite3.Connection, secret: bytes, apply_event):
        self.conn = conn
        self.secret = secret
        self.apply_event = apply_event

    def handle(
        self,
        raw_body: bytes,
        headers: dict[str, str],
        now_seconds: int | None = None,
    ) -> tuple[int, dict]:
        now = int(time.time()) if now_seconds is None else now_seconds
        lowered = {key.lower(): value for key, value in headers.items()}

        if not verify_signature(
            self.secret,
            lowered.get("x-fluxapay-signature"),
            lowered.get("x-fluxapay-timestamp"),
            raw_body,
            now,
        ):
            return 401, {"error": "invalid signature"}

        try:
            event = json.loads(raw_body)
        except ValueError:
            return 400, {"error": "invalid JSON"}

        # The envelope's ``event_id`` is stable across retries; ``id`` is the
        # delivery id and ``data.payment_id`` is not unique per event.
        event_id = event.get("event_id") if isinstance(event, dict) else None
        if not isinstance(event_id, str) or not event_id:
            return 400, {"error": "missing event_id"}

        data = event.get("data") or {}
        resource_id = resource_id_of(data)
        if not resource_id:
            return 400, {"error": "missing payment_id/refund_id/dispute_id"}

        # Ordering that survives a crash: claim and side effect commit together.
        # - Crash before COMMIT: both roll back and the retry processes normally.
        # - Crash after COMMIT: the retry finds the row and is acknowledged as a
        #   duplicate instead of applying the side effect twice.
        self.conn.execute("BEGIN IMMEDIATE")
        try:
            cursor = self.conn.execute(
                """
                INSERT INTO processed_webhook_events
                    (event_id, event_type, resource_id, received_at)
                VALUES (?, ?, ?, ?)
                ON CONFLICT (event_id) DO NOTHING
                """,
                (event_id, str(event.get("type", "")), str(resource_id), now),
            )
            if cursor.rowcount == 0:
                self.conn.execute("COMMIT")
                return 200, {"received": True, "duplicate": True}

            # Test deliveries are signed like real ones but must not move money.
            if event.get("livemode") is not False:
                self.apply_event(self.conn, event)

            self.conn.execute("COMMIT")
        except Exception:
            self.conn.execute("ROLLBACK")
            # Propagate so the HTTP layer answers 5xx and FluxaPay retries.
            raise

        self.conn.execute(
            "DELETE FROM processed_webhook_events WHERE received_at < ?",
            (now - RETENTION_SECONDS,),
        )
        return 200, {"received": True, "duplicate": False}


def mark_payment_confirmed(conn: sqlite3.Connection, event: dict) -> None:
    """Example side effect: mark the payment confirmed in your own tables."""
    if event.get("type") != "payment.confirmed":
        return
    data = event.get("data") or {}
    payment_id = data.get("payment_id")
    if not payment_id:
        return
    conn.execute(
        """
        INSERT INTO confirmed_payments
            (payment_id, amount, currency, confirmed_at)
        VALUES (?, ?, ?, ?)
        ON CONFLICT (payment_id) DO NOTHING
        """,
        (
            payment_id,
            str(data.get("amount", "")),
            str(data.get("currency", "")),
            str(event.get("createdAt", "")),
        ),
    )


def create_app(secret: bytes, conn: sqlite3.Connection) -> FastAPI:
    app = FastAPI()
    receiver = WebhookReceiver(conn, secret, mark_payment_confirmed)

    @app.post("/webhooks/fluxapay")
    async def fluxapay_webhook(
        request: Request,
        x_fluxapay_signature: str = Header(default=""),
        x_fluxapay_timestamp: str = Header(default=""),
    ):
        status, body = receiver.handle(
            await request.body(),
            {
                "x-fluxapay-signature": x_fluxapay_signature,
                "x-fluxapay-timestamp": x_fluxapay_timestamp,
            },
        )
        return JSONResponse(status_code=status, content=body)

    return app


def _default_app() -> FastAPI:
    secret = os.environ.get("FLUXAPAY_WEBHOOK_SECRET", "")
    if not secret:
        raise RuntimeError("FLUXAPAY_WEBHOOK_SECRET is required")
    conn = connect(os.environ.get("FLUXAPAY_DB", "webhooks.db"))
    conn.execute(
        """
        CREATE TABLE IF NOT EXISTS confirmed_payments (
            payment_id   TEXT PRIMARY KEY,
            amount       TEXT NOT NULL,
            currency     TEXT NOT NULL,
            confirmed_at TEXT NOT NULL
        )
        """
    )
    return create_app(secret.encode(), conn)


app = _default_app()
