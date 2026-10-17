"""Async and synchronous FluxaPay API clients built on httpx."""

from __future__ import annotations

import asyncio
from typing import Any, Dict, Optional

import httpx

from .errors import FluxaPayError, error_from_response
from .types import (
    CreatePaymentRequest,
    CreateRefundRequest,
    Payment,
    Refund,
    VerifyPaymentResponse,
)

DEFAULT_BASE_URL = "https://api.fluxapay.com/v1"
RETRY_STATUS_CODES = frozenset({429, 500, 502, 503, 504})


class AsyncFluxaPayClient:
    """Asynchronous FluxaPay API client using httpx."""

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        max_retries: int = 3,
        timeout: float = 30.0,
        client: Optional[httpx.AsyncClient] = None,
    ) -> None:
        if not api_key:
            raise ValueError("api_key is required")
        self.api_key = api_key
        self.base_url = base_url.rstrip("/")
        self.max_retries = max_retries
        self.timeout = timeout
        self._client = client
        self._owns_client = client is None

    def _headers(self) -> Dict[str, str]:
        return {
            "Authorization": f"Bearer {self.api_key}",
            "Content-Type": "application/json",
            "Accept": "application/json",
        }

    async def _get_client(self) -> httpx.AsyncClient:
        if self._client is None:
            self._client = httpx.AsyncClient(base_url=self.base_url, timeout=self.timeout)
        return self._client

    async def _request(self, method: str, path: str, json: Optional[Dict[str, Any]] = None) -> Dict[str, Any]:
        client = await self._get_client()
        last_error: Optional[Exception] = None

        for attempt in range(self.max_retries + 1):
            try:
                response = await client.request(method, path, json=json, headers=self._headers())
            except httpx.HTTPError as exc:
                last_error = exc
                if attempt >= self.max_retries:
                    raise FluxaPayError(f"Request failed: {exc}") from exc
                await asyncio.sleep(2 ** attempt)
                continue

            if response.status_code in RETRY_STATUS_CODES and attempt < self.max_retries:
                await asyncio.sleep(2 ** attempt)
                continue

            if response.status_code >= 400:
                try:
                    payload = response.json()
                except ValueError:
                    payload = response.text
                message = payload.get("message") if isinstance(payload, dict) else str(payload)
                raise error_from_response(response.status_code, message or "Request failed", payload)

            if response.status_code == 204 or not response.content:
                return {}
            return response.json()

        raise FluxaPayError(f"Request failed after retries: {last_error}")

    async def create_payment(self, request: CreatePaymentRequest | Dict[str, Any]) -> Payment:
        if isinstance(request, dict):
            request = CreatePaymentRequest(**request)
        data = await self._request("POST", "/payments", json=request.model_dump(by_alias=True, exclude_none=True))
        return Payment(**data)

    async def get_payment(self, payment_id: str) -> Payment:
        data = await self._request("GET", f"/payments/{payment_id}")
        return Payment(**data)

    async def verify_payment(self, payment_id: str) -> VerifyPaymentResponse:
        data = await self._request("GET", f"/payments/{payment_id}/verify")
        return VerifyPaymentResponse(**data)

    async def create_refund(self, request: CreateRefundRequest | Dict[str, Any]) -> Refund:
        if isinstance(request, dict):
            request = CreateRefundRequest(**request)
        data = await self._request("POST", "/refunds", json=request.model_dump(by_alias=True, exclude_none=True))
        return Refund(**data)

    async def aclose(self) -> None:
        if self._client is not None and self._owns_client:
            await self._client.aclose()
            self._client = None

    async def __aenter__(self) -> "AsyncFluxaPayClient":
        return self

    async def __aexit__(self, *exc: Any) -> None:
        await self.aclose()

    def __repr__(self) -> str:
        return f"AsyncFluxaPayClient(base_url={self.base_url!r}, max_retries={self.max_retries!r})"


class FluxaPayClient:
    """Synchronous wrapper around :class:`AsyncFluxaPayClient`."""

    def __init__(
        self,
        api_key: str,
        *,
        base_url: str = DEFAULT_BASE_URL,
        max_retries: int = 3,
        timeout: float = 30.0,
    ) -> None:
        self._async = AsyncFluxaPayClient(
            api_key, base_url=base_url, max_retries=max_retries, timeout=timeout
        )

    def _run(self, coro: Any) -> Any:
        try:
            asyncio.get_running_loop()
        except RuntimeError:
            return asyncio.run(coro)
        raise FluxaPayError(
            "FluxaPayClient cannot be used inside a running event loop; use AsyncFluxaPayClient"
        )

    def create_payment(self, request: CreatePaymentRequest | Dict[str, Any]) -> Payment:
        return self._run(self._async.create_payment(request))

    def get_payment(self, payment_id: str) -> Payment:
        return self._run(self._async.get_payment(payment_id))

    def verify_payment(self, payment_id: str) -> VerifyPaymentResponse:
        return self._run(self._async.verify_payment(payment_id))

    def create_refund(self, request: CreateRefundRequest | Dict[str, Any]) -> Refund:
        return self._run(self._async.create_refund(request))

    def close(self) -> None:
        self._run(self._async.aclose())

    def __enter__(self) -> "FluxaPayClient":
        return self

    def __exit__(self, *exc: Any) -> None:
        self.close()

    def __repr__(self) -> str:
        return f"FluxaPayClient(base_url={self._async.base_url!r}, max_retries={self._async.max_retries!r})"
