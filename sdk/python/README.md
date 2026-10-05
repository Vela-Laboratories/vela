# Vela Python client

This standard-library client only reads `/v1` routes. Add `sdk/python` to
`PYTHONPATH` or install it from this checkout once packaging is available.

```python
from vela import VelaClient

client = VelaClient()
for market in client.markets():
    print(market["id"])
```

Account balances and portfolio reads require a short-lived bearer session:
`VelaClient(bearer_token=session_token)`. The client has no order, withdrawal,
or signing methods.

## Private history and execution progress

With the master account's bearer session, `orders(address)`, `fills(address)`,
`algos(address)` and `algo(parent_id)` read open orders, recent fills and TWAP
progress. Fill reads currently return the most recent 500 records. Decimal
prices and quantities in order/fill history remain strings; fixed-point TWAP
quantities are Python integers.

The Python client remains read-only. It cannot create or cancel a parent.
`VelaClientError.status` and `.retry_after_seconds` preserve HTTP failure
metadata. A configured positive `timeout_seconds` bounds reads; requests are
never retried automatically. URLs containing credentials, queries or fragments
are rejected before bearer authentication is attached.

Run tests with:

```sh
PYTHONPATH=sdk/python python -m unittest discover -s sdk/python/tests
```
