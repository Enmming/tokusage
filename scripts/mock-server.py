#!/usr/bin/env python3
"""
Minimal mock server for `tokusage submit` during Phase 1.

Listens on 127.0.0.1:8080, accepts `POST /api/submit` with
`Authorization: Bearer ...`. Writes each received payload to
/tmp/tokusage-mock/<timestamp>-<host_id>.json and prints a summary.
"""
import http.server
import json
import os
import sys
from datetime import datetime

OUT = "/tmp/tokusage-mock"
os.makedirs(OUT, exist_ok=True)


class Handler(http.server.BaseHTTPRequestHandler):
    def do_POST(self):
        if self.path != "/api/submit":
            self.send_error(404)
            return
        auth = self.headers.get("Authorization", "")
        if not auth.startswith("Bearer "):
            self.send_error(401, "missing Bearer token")
            return
        length = int(self.headers.get("Content-Length", 0))
        body = self.rfile.read(length)
        try:
            payload = json.loads(body)
        except Exception as e:
            self.send_error(400, f"invalid JSON: {e}")
            return

        host_id = payload.get("meta", {}).get("host_id", "unknown")
        ts = datetime.utcnow().strftime("%Y%m%dT%H%M%S%f")
        name = f"{ts}-{host_id}.json"
        path = os.path.join(OUT, name)
        with open(path, "w") as f:
            json.dump(payload, f, indent=2)

        contrib = len(payload.get("contributions", []))
        dr = payload.get("meta", {}).get("date_range", {})
        total_tokens = sum(
            sum(c.get("tokens", {}).values()) for c in payload.get("contributions", [])
        )
        total_cost = sum(c.get("cost_cents", 0.0) for c in payload.get("contributions", []))
        print(
            f"[{ts}] host={host_id} contributions={contrib} "
            f"range={dr.get('start')}..{dr.get('end')} "
            f"tokens={total_tokens} cost_cents={total_cost:.2f} saved={path}"
        )

        resp = json.dumps({"ok": True, "saved_as": name}).encode()
        self.send_response(200)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(resp)))
        self.end_headers()
        self.wfile.write(resp)

    def log_message(self, *_):
        pass  # suppress default access log; we print our own summary


def main():
    port = int(sys.argv[1]) if len(sys.argv) > 1 else 8080
    print(f"tokusage mock server listening on http://127.0.0.1:{port}")
    print(f"payloads will be saved under {OUT}")
    http.server.HTTPServer(("127.0.0.1", port), Handler).serve_forever()


if __name__ == "__main__":
    main()
