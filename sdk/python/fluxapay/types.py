"""Pydantic v2 models for the FluxaPay Python SDK.

All models are JSON-serializable via ``model_dump_json`` and provide a
readable ``__repr__`` for debugging.
"""

from __future__ import annotations

from datetime import datetime
from enum import Enum
from typing import Any, Dict, List, Optional

from pydantic import BaseModel, ConfigDict, Field, field_validator


class _BaseModel(BaseModel):
    """Shared base: forbid unknown fields and expose a compact repr."""

    model_config = ConfigDict(extra="ignore", populate_by_name=True)

    def __repr__(self) -> str:  # pragma: no cover - trivial
        fields = ", ".join(
            f"{name}={value!r}" for name, value in self.__dict__.items()
        )
        return f"{type(self).__name__}({fields})"


class PaymentStatus(str, Enum):
    PENDING = "pending"
    CONFIRMED = "confirmed"
    FAILED = "failed"
    EXPIRED = "expired"
    REFUNDED = "refunded"


class RefundStatus(str, Enum):
    PENDING = "pending"
    COMPLETED = "completed"
    FAILED = "failed"


class CreatePaymentRequest(_BaseModel):
    amount: float = Field(..., gt=0, description="Payment amount in the asset's units")
    currency: str = Field(..., min_length=3, max_length=10)
    asset: str = Field(default="USDC", min_length=2, max_length=10)
    reference: Optional[str] = Field(default=None, max_length=128)
    callback_url: Optional[str] = Field(default=None, alias="callbackUrl")
    metadata: Optional[Dict[str, Any]] = None

    @field_validator("currency", "asset")
    @classmethod
    def _upper(cls, value: str) -> str:
        return value.upper()


class Payment(_BaseModel):
    id: str
    amount: float
    currency: str
    asset: str
    status: PaymentStatus
    reference: Optional[str] = None
    checkout_url: Optional[str] = Field(default=None, alias="checkoutUrl")
    created_at: Optional[datetime] = Field(default=None, alias="createdAt")
    metadata: Optional[Dict[str, Any]] = None


class VerifyPaymentResponse(_BaseModel):
    id: str
    status: PaymentStatus
    verified: bool
    amount: Optional[float] = None
    tx_hash: Optional[str] = Field(default=None, alias="txHash")


class CreateRefundRequest(_BaseModel):
    payment_id: str = Field(..., alias="paymentId", min_length=1)
    amount: Optional[float] = Field(default=None, gt=0)
    reason: Optional[str] = Field(default=None, max_length=256)


class Refund(_BaseModel):
    id: str
    payment_id: str = Field(..., alias="paymentId")
    amount: float
    status: RefundStatus
    reason: Optional[str] = None
    created_at: Optional[datetime] = Field(default=None, alias="createdAt")


class WebhookEvent(_BaseModel):
    id: str
    type: str
    data: Dict[str, Any]
    created_at: Optional[datetime] = Field(default=None, alias="createdAt")


class ErrorResponse(_BaseModel):
    code: str
    message: str
    details: Optional[List[Dict[str, Any]]] = None
