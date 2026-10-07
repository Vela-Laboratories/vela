"""Verify that the release smoke test checks the running commit."""

import unittest
from unittest.mock import patch

from scripts.post_deploy_smoke import check


class PostDeploySmokeTests(unittest.TestCase):
    def test_accepts_only_the_expected_running_build(self) -> None:
        responses = {
            "/health": (200, "ok"),
            "/ready": (200, {"ok": True}),
            "/status": (200, {"data": {"build_id": "approved-sha"}}),
            "/markets": (200, {"ok": True, "data": [{"id": "BTC-USDC"}]}),
            "/openapi.json": (200, {"paths": {"/markets": {}}}),
        }
        with patch("scripts.post_deploy_smoke.fetch", side_effect=lambda _, path: responses[path]):
            check("https://example.invalid", "approved-sha")
            with self.assertRaisesRegex(ValueError, "build does not match"):
                check("https://example.invalid", "older-sha")


if __name__ == "__main__":
    unittest.main()
