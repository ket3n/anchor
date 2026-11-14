import unittest
from client import http, retry


class HttpTest(unittest.TestCase):
    def test_get(self):
        self.assertEqual(http.get("u", lambda u: "ok:" + u), "ok:u")

    def test_get_many(self):
        self.assertEqual(http.get_many(["a", "b"], lambda u: u.upper()), ["A", "B"])

    def test_retry_eventually(self):
        calls = []
        def flaky():
            calls.append(1)
            if len(calls) < 3:
                raise IOError("x")
            return "done"
        self.assertEqual(retry.retry_with_backoff(flaky, sleep=lambda s: None), "done")


if __name__ == "__main__":
    unittest.main()
