#!/usr/bin/env python3
"""
Minimal mock server for `tokusage submit`.

Listens on 127.0.0.1:8080, accepts `POST /api/submit` with
`Authorization: Bearer ...`. Writes each received payload to
/tmp/tokusage-mock/<timestamp>-events-<count>.json and prints a summary.
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

        events = payload.get("events", [])
        ts = datetime.utcnow().strftime("%Y%m%dT%H%M%S%f")
        name = f"{ts}-events-{len(events)}.json"
        path = os.path.join(OUT, name)
        with open(path, "w") as f:
            json.dump(payload, f, indent=2)

        total_tokens = sum(
            sum(event.get("tokens", {}).values()) for event in events
        )
        total_cost = sum(event.get("cost_cents", 0.0) for event in events)
        print(
            f"[{ts}] events={len(events)} client_version={payload.get('client_version')} "
            f"submitted_at={payload.get('submitted_at')} "
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
