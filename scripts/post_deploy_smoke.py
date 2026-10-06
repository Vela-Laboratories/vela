#!/usr/bin/env python3
"""Read-only checks for the public testnet API after a deployment."""

import argparse
import json
import sys
import time
from urllib.error import HTTPError, URLError
from urllib.request import urlopen


def fetch(base_url: str, path: str) -> tuple[int, object]:
    try:
        with urlopen(f"{base_url.rstrip('/')}{path}", timeout=10) as response:
            body = response.read()
            status = response.status
    except HTTPError as error:
        status = error.code
        body = error.read()
    if path == "/health":
        return status, body.decode("utf-8")
    return status, json.loads(body)


def check(base_url: str, expected_build_id: str) -> None:
    status, health = fetch(base_url, "/health")
    if status != 200 or health != "ok":
        raise ValueError(f"health failed: HTTP {status}")

    status, ready = fetch(base_url, "/ready")
    if status != 200 or not isinstance(ready, dict) or ready.get("ok") is not True:
        raise ValueError(f"readiness failed: HTTP {status}")

    status, build = fetch(base_url, "/status")
    if (status != 200 or not isinstance(build, dict)
            or not isinstance(build.get("data"), dict)
            or build["data"].get("build_id") != expected_build_id):
        raise ValueError("running API build does not match the deployed commit")

    status, markets = fetch(base_url, "/markets")
    if (status != 200 or not isinstance(markets, dict)
            or markets.get("ok") is not True
            or not isinstance(markets.get("data"), list)
            or not markets["data"]):
        raise ValueError(f"market catalog failed: HTTP {status}")

    status, spec = fetch(base_url, "/openapi.json")
    if (status != 200 or not isinstance(spec, dict)
            or not isinstance(spec.get("paths"), dict)
            or "/markets" not in spec["paths"]):
        raise ValueError(f"API schema failed: HTTP {status}")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("base_url", help="Public API origin, for example https://vela-engine.fly.dev")
    parser.add_argument("--expected-build-id", required=True, help="Git commit approved by CI")
    parser.add_argument("--attempts", type=int, default=6)
    args = parser.parse_args()
    for attempt in range(1, args.attempts + 1):
        try:
            check(args.base_url, args.expected_build_id)
            print(f"Post-deploy smoke checks passed for {args.base_url}")
            return 0
        except (ValueError, URLError, TimeoutError, json.JSONDecodeError) as error:
            print(f"Attempt {attempt}/{args.attempts}: {error}", file=sys.stderr)
            if attempt < args.attempts:
                time.sleep(5)
    return 1


if __name__ == "__main__":
    raise SystemExit(main())
