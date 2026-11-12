#!/usr/bin/env python3
"""openai-relay.py [PORT]: a local relay to OpenAI's API for Gary (docs/GARY.md).

Gary sends `max_tokens`, which OpenAI's newer models refuse ("Use 'max_completion_tokens'
instead"); this renames it and passes everything else through unchanged, both ways. Gary's
OpenAI engine points at http://127.0.0.1:PORT/v1 (8090 unless given). The request's
Authorization header (the key, in Gary's settings) is passed on as it is and never logged or
kept. Only requests to the local address are taken.
"""
import json
import sys
import urllib.error
import urllib.request
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

UPSTREAM = "https://api.openai.com"
PASSED = ("Authorization", "Content-Type", "OpenAI-Organization", "OpenAI-Project")


class Relay(BaseHTTPRequestHandler):
    def _relay(self, body):
        headers = {h: self.headers[h] for h in PASSED if self.headers.get(h)}
        req = urllib.request.Request(UPSTREAM + self.path, data=body, headers=headers,
                                     method=self.command)
        try:
            with urllib.request.urlopen(req, timeout=120) as r:
                status, data, ctype = r.status, r.read(), r.headers.get("Content-Type", "")
        except urllib.error.HTTPError as e:
            status, data, ctype = e.code, e.read(), e.headers.get("Content-Type", "")
        except urllib.error.URLError as e:
            status, data, ctype = 502, json.dumps({"error": {"message": str(e.reason)}}).encode(), "application/json"
        self.send_response(status)
        self.send_header("Content-Type", ctype or "application/json")
        self.send_header("Content-Length", str(len(data)))
        self.end_headers()
        self.wfile.write(data)
        # (what passed, never the headers: the key's in them)
        sys.stderr.write(f"{self.command} {self.path} -> {status}\n")

    def do_GET(self):
        self._relay(None)

    def do_POST(self):
        body = self.rfile.read(int(self.headers.get("Content-Length") or 0))
        try:
            d = json.loads(body)
            if isinstance(d, dict) and "max_tokens" in d:
                d.setdefault("max_completion_tokens", d.pop("max_tokens"))
                body = json.dumps(d).encode()
        except ValueError:
            pass
        self._relay(body)

    def log_message(self, *_):
        pass  # (the default log line has nothing secret, but _relay says what's needed)


if __name__ == "__main__":
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8090
    print(f"relaying http://127.0.0.1:{port} -> {UPSTREAM} (max_tokens renamed)", flush=True)
    ThreadingHTTPServer(("127.0.0.1", port), Relay).serve_forever()
