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
