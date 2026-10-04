"""Typed, read-only access to Vela's versioned HTTP API."""

from __future__ import annotations

import json
from typing import Literal, NotRequired, TypedDict, cast
from urllib.error import HTTPError, URLError
from urllib.parse import quote, urlencode
from urllib.request import Request, urlopen


class VelaClientError(RuntimeError):
    """The API or network could not satisfy a read request."""


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
        except (HTTPError, URLError, OSError, ValueError) as error:
            raise VelaClientError(f"Vela read failed: {error}") from error
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
