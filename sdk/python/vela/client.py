"""Typed, read-only access to Vela's versioned HTTP API."""

from __future__ import annotations

import json
import math
from typing import Literal, NotRequired, TypedDict, cast
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode, urlsplit
from urllib.request import Request, urlopen


class VelaClientError(RuntimeError):
    """The API or network could not satisfy a read request."""

    def __init__(self, message: str, status: int | None = None, retry_after_seconds: int | None = None) -> None:
        super().__init__(message)
        self.status = status
        self.retry_after_seconds = retry_after_seconds


class Market(TypedDict):
    id: str
    base: str
    quote: str
    best_bid: str | None
    best_ask: str | None
    spread: str | None


class BookLevel(TypedDict):
    price: str
    quantity: str


class Book(TypedDict):
    market: str
    bids: list[BookLevel]
    asks: list[BookLevel]


class Portfolio(TypedDict):
    address: str
    cost_basis_method: Literal["FIFO", "HIFO"]
    markets: list[dict[str, object]]
    tax_lots: list[dict[str, object]]
    total_realized_pnl_usdc: str
    total_unrealized_pnl_usdc: str
    total_pnl_usdc: str


class AgentScope(TypedDict):
    allowed_markets: list[str] | None
    allowed_order_types: list[str] | None
    allowed_sides: list[str] | None
    max_notional_per_hour: int | None
    max_notional_per_day: int | None


class Agent(TypedDict):
    agent: str
    expires_at_ms: int
    max_notional_per_order: int
    revoked: bool
    active: bool
    nonce: int
    scope: AgentScope


class AgentList(TypedDict):
    master: str
    agents: list[Agent]


class AccountFill(TypedDict):
    id: str
    market: str
    price: str
    quantity: str
    order_id: int
    counterparty_order_id: int
    role: Literal["maker", "taker"]
    side: Literal["buy", "sell"]
    timestamp: int


class AlgoSnapshot(TypedDict):
    parent_id: int
    user: str
    market: str
    side: str
    total_quantity: int
    filled_quantity: int
    remaining_quantity: int
    price_limit: int | None
    duration_secs: int
    slices: int
    started_at_ms: int
    status: Literal["Running", "Completed", "Incomplete", "Canceled", "Failed"]


class ApiEnvelope(TypedDict):
    ok: bool
    data: NotRequired[object]
    error: NotRequired[str]


class VelaClient:
    """Uses GET only; customer reads require a short-lived bearer session."""

    def __init__(
        self,
        base_url: str = "https://vela-engine.fly.dev",
        *,
        bearer_token: str | None = None,
        timeout_seconds: float = 10.0,
    ) -> None:
        parsed = urlsplit(base_url)
        if parsed.scheme not in ("http", "https") or not parsed.hostname or parsed.username is not None or parsed.password is not None or parsed.query or parsed.fragment:
            raise ValueError("base_url must be an HTTP origin without credentials, query or fragment")
        if not math.isfinite(timeout_seconds) or timeout_seconds <= 0:
            raise ValueError("timeout_seconds must be finite and positive")
        self.base_url = base_url.rstrip("/")
        self.bearer_token = bearer_token
        self.timeout_seconds = timeout_seconds

    def _get(self, path: str) -> object:
        headers = {"Accept": "application/json"}
        if self.bearer_token:
            headers["Authorization"] = f"Bearer {self.bearer_token}"
        request = Request(f"{self.base_url}/v1{path}", headers=headers, method="GET")
        try:
            with urlopen(request, timeout=self.timeout_seconds) as response:
                payload: ApiEnvelope = json.load(response)
        except HTTPError as error:
            retry_header = error.headers.get("Retry-After", "") if error.headers else ""
            retry_after = int(retry_header) if retry_header.isascii() and retry_header.isdigit() else None
            message = f"Vela API returned HTTP {error.code}"
            try:
                envelope = json.loads(error.read(16_384))
                if isinstance(envelope, dict) and isinstance(envelope.get("error"), str):
                    message = envelope["error"][:512]
            except (ValueError, OSError):
                pass
            finally:
                error.close()
            raise VelaClientError(message, error.code, retry_after) from error
        except (URLError, OSError, ValueError) as error:
            raise VelaClientError("Vela read failed: network or invalid JSON response") from error
        if not isinstance(payload, dict) or payload.get("ok") is not True or "data" not in payload:
            raise VelaClientError("Vela returned an unsuccessful or malformed response")
        return payload["data"]

    def markets(self) -> list[Market]:
        return cast(list[Market], self._get("/markets"))

    def book(self, market: str) -> Book:
        return cast(Book, self._get(f"/markets/{quote(market, safe='')}/book"))

    def portfolio(self, address: str, method: Literal["fifo", "hifo"] = "fifo") -> Portfolio:
        query = urlencode({"method": method})
        return cast(Portfolio, self._get(f"/portfolio/{quote(address, safe='')}?{query}"))

    def agents(self, master: str) -> AgentList:
        return cast(AgentList, self._get(f"/agents/{quote(master, safe='')}"))

    def balances(self, address: str) -> list[dict[str, object]]:
        return cast(list[dict[str, object]], self._get(f"/account/{quote(address, safe='')}/balances"))

    def orders(self, address: str) -> list[dict[str, object]]:
        return cast(list[dict[str, object]], self._get(f"/account/{quote(address, safe='')}/orders"))

    def fills(self, address: str) -> list[AccountFill]:
        return cast(list[AccountFill], self._get(f"/account/{quote(address, safe='')}/fills"))

    def algos(self, address: str) -> list[AlgoSnapshot]:
        return cast(list[AlgoSnapshot], self._get(f"/account/{quote(address, safe='')}/algos"))

    def algo(self, parent_id: int) -> AlgoSnapshot:
        if type(parent_id) is not int or not 0 <= parent_id <= 2**64 - 1:
            raise ValueError("parent_id must be an unsigned 64-bit integer")
        return cast(AlgoSnapshot, self._get(f"/orders/algo/{parent_id}"))
