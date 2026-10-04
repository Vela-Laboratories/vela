import io
import json
import unittest
from unittest.mock import patch

from vela import VelaClient, VelaClientError


class Response(io.BytesIO):
    def __enter__(self):
        return self

    def __exit__(self, *_args):
        self.close()


class ClientTests(unittest.TestCase):
    def test_reads_versioned_markets_without_write_method(self):
        payload = Response(json.dumps({"ok": True, "data": [{"id": "ETH-USDC"}]}).encode())
        with patch("vela.client.urlopen", return_value=payload) as urlopen:
            markets = VelaClient(base_url="https://example.test/").markets()
        self.assertEqual(markets[0]["id"], "ETH-USDC")
        request = urlopen.call_args.args[0]
        self.assertEqual(request.full_url, "https://example.test/v1/markets")
        self.assertEqual(request.get_method(), "GET")

    def test_customer_read_sends_bearer_session(self):
        payload = Response(json.dumps({"ok": True, "data": []}).encode())
        with patch("vela.client.urlopen", return_value=payload) as urlopen:
            VelaClient(bearer_token="test-session").balances("0xabc")
        self.assertEqual(urlopen.call_args.args[0].get_header("Authorization"), "Bearer test-session")

    def test_failed_envelope_raises(self):
        payload = Response(json.dumps({"ok": False, "error": "denied"}).encode())
        with patch("vela.client.urlopen", return_value=payload):
            with self.assertRaises(VelaClientError):
                VelaClient().markets()


if __name__ == "__main__":
    unittest.main()
