import io
import json
import unittest
from email.message import Message
from urllib.error import HTTPError
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

    def test_http_error_keeps_status_retry_and_body(self):
        headers = Message(); headers["Retry-After"] = "7"
        error = HTTPError("https://example.test/v1/markets", 429, "Too Many Requests", headers, Response(b'{"ok":false,"error":"rate limited"}'))
        with patch("vela.client.urlopen", side_effect=error) as transport:
            with self.assertRaises(VelaClientError) as caught:
                VelaClient().markets()
        self.assertEqual(caught.exception.status, 429)
        self.assertEqual(caught.exception.retry_after_seconds, 7)
        self.assertEqual(str(caught.exception), "rate limited")
        self.assertEqual(transport.call_count, 1)

    def test_history_reads_encode_paths_and_use_get(self):
        for method in ["orders", "fills", "algos"]:
            with patch("vela.client.urlopen", return_value=Response(b'{"ok":true,"data":[]}')) as transport:
                getattr(VelaClient(), method)("0xabc/?")
            request = transport.call_args.args[0]
            self.assertIn("0xabc%2F%3F", request.full_url)
            self.assertEqual(request.get_method(), "GET")

    def test_config_and_parent_ids_fail_before_transport(self):
        for url in ["file:///tmp/example", "https://user:password@example.test", "https://example.test?token=test"]:
            with self.assertRaises(ValueError): VelaClient(base_url=url)
        for timeout in [0, -1, float("inf"), float("nan")]:
            with self.assertRaises(ValueError): VelaClient(timeout_seconds=timeout)
        for parent in [-1, True, 2**64, 1.5]:
            with self.assertRaises(ValueError): VelaClient().algo(parent)

    def test_malformed_json_and_envelopes_fail_closed(self):
        for payload in [b'not-json', b'[]', b'{"ok":true}']:
            with patch("vela.client.urlopen", return_value=Response(payload)):
                with self.assertRaises(VelaClientError): VelaClient().markets()

    def test_transport_receives_configured_timeout(self):
        with patch("vela.client.urlopen", return_value=Response(b'{"ok":true,"data":[]}')) as transport:
            VelaClient(timeout_seconds=7.5).markets()
        self.assertEqual(transport.call_args.kwargs["timeout"], 7.5)


if __name__ == "__main__":
    unittest.main()
