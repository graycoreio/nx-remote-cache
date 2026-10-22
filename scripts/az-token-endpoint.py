#!/usr/bin/env python3
"""A local stand-in for Azure's managed-identity token endpoint, backed by `az login`.

Lets the nx-remote-cache container authenticate to Azure Storage as the developer, without the
Azure CLI or any keys inside the server image. Run it next to the server (see
docker-compose.dev.yml), and start the server with `auth = { mode = "managed_identity" }` plus:

    IDENTITY_ENDPOINT=http://<this host>:<port>/metadata/identity/oauth2/token
    IDENTITY_HEADER=<the same secret as below>

It speaks the App Service managed-identity protocol (api-version 2019-08-01) that the Azure SDKs
use: GET ?resource=<resource>, header `x-identity-header: <secret>`, JSON response with
`access_token`, `expires_on` (unix seconds as a string), `token_type` and `resource`.

Environment:
    IDENTITY_HEADER    required; requests must present it
    ALLOWED_RESOURCES  comma-separated, default https://storage.azure.com: tokens are only
                       issued for these, so other containers can't obtain e.g. ARM tokens
    PORT               default 8080

Standard library only, so it runs on the official mcr.microsoft.com/azure-cli image as is.
"""

import hmac
import json
import os
import subprocess
import sys
import threading
import time
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from urllib.parse import parse_qs, urlparse

SECRET = os.environ.get("IDENTITY_HEADER", "")
ALLOWED = {
    r.strip().rstrip("/")
    for r in os.environ.get("ALLOWED_RESOURCES", "https://storage.azure.com").split(",")
    if r.strip()
}
PORT = int(os.environ.get("PORT", "8080"))
# Re-run `az` this long before a cached token expires.
REFRESH_MARGIN = 300

_cache: dict[str, tuple[str, int]] = {}
_lock = threading.Lock()


def token_for(resource: str) -> tuple[str, int]:
    with _lock:
        cached = _cache.get(resource)
        if cached and cached[1] - REFRESH_MARGIN > time.time():
            return cached
        result = subprocess.run(
            ["az", "account", "get-access-token", "--resource", resource, "--output", "json"],
            capture_output=True,
            text=True,
            timeout=60,
        )
        if result.returncode != 0:
            raise RuntimeError(result.stderr.strip() or "az account get-access-token failed")
        data = json.loads(result.stdout)
        token = (data["accessToken"], int(data["expires_on"]))
        _cache[resource] = token
        return token


class Handler(BaseHTTPRequestHandler):
    server_version = "az-token-endpoint"

    def _reply(self, status: int, body: dict) -> None:
        payload = json.dumps(body).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self) -> None:  # noqa: N802 (http.server naming)
        url = urlparse(self.path)
        if url.path == "/healthz":
            return self._reply(200, {"status": "ok"})

        presented = self.headers.get("x-identity-header", "")
        if not SECRET or not hmac.compare_digest(presented.encode(), SECRET.encode()):
            return self._reply(401, {"error": "invalid or missing x-identity-header"})

        resource = (parse_qs(url.query).get("resource") or [""])[0].rstrip("/")
        if resource not in ALLOWED:
            return self._reply(
                400, {"error": f"resource {resource!r} is not allowed (ALLOWED_RESOURCES)"}
            )
        try:
            access_token, expires_on = token_for(resource)
        except Exception as e:  # surfaced to the server, which logs it
            return self._reply(500, {"error": f"{e} (is `az login` done in this container?)"})
        self._reply(
            200,
            {
                "access_token": access_token,
                "expires_on": str(expires_on),
                "token_type": "Bearer",
                "resource": resource,
            },
        )

    def log_message(self, fmt: str, *args) -> None:
        # Request line and status only; never tokens or headers.
        sys.stderr.write(f"{self.address_string()} {fmt % args}\n")


def main() -> None:
    if not SECRET:
        sys.exit("IDENTITY_HEADER must be set")
    print(f"token endpoint on :{PORT}, resources: {', '.join(sorted(ALLOWED))}", flush=True)
    ThreadingHTTPServer(("0.0.0.0", PORT), Handler).serve_forever()


if __name__ == "__main__":
    main()
