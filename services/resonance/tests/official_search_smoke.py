"""Pinned-image search boundaries in an owned disposable local stack.

No official credentials, remote upstream, retained data or native client is used.
Cold positive results/cache/authority races are verified by loopback PG fixtures.
"""
import json
import os
import urllib.parse

from protocol_smoke import Client


def run():
    assert os.environ.get("ILLYA_ALLOW_DISPOSABLE_TEST") == "1", "Disposable guard required"
    admin, player, anonymous = (Client(os.environ["TEST_SANCTUARY_URL"]) for _ in range(3))
    for client, prefix in ((admin, "ADMIN"), (player, "PLAYER")):
        client.json("POST", "/api/v1/auth/login", {
            "username": os.environ[f"TEST_{prefix}_USERNAME"],
            "password": os.environ[f"TEST_{prefix}_PASSWORD"],
        })
    path = "/api/v1/beatmaps/official/search"
    before = anonymous.json("GET", "/api/v1/beatmaps?page_size=1")
    assert before["total"] == 0, "Fresh disposable catalogue required"

    def error(client, query, status, code):
        suffix = "?" + urllib.parse.urlencode(query) if query else ""
        result = client.json("GET", path + suffix, expected=status)
        assert result["error"]["code"] == code, "Unexpected sanitized search boundary"
        assert "token" not in result["error"]["message"].lower(), "Credential leakage"

    # Authentication and distinct permission precede even malformed parameters.
    for query in ({}, {"mode": "5"}, {"unrecognized": "value"}):
        error(anonymous, query, 401, "unauthorized")
        error(player, query, 403, "forbidden")
    invalid_queries = [
        {"mode": "5"}, {"mode": "-1"}, {"mode": "osu"},
        {"status": "mine"}, {"sort": "external_url"}, {"unrecognized": "value"},
        {"q": "x" * 201}, {"q": "hello\x00world"},
        {"cursor_string": "x" * 2049}, {"cursor_string": "line\nbreak"},
    ]
    for query in invalid_queries:
        error(admin, query, 400, "invalid_request")
    # Opaque cursor characters are valid input; none becomes a URL or field.
    for query in ({}, {"q": "Synthetic search", "mode": "3", "status": "any",
                       "sort": "date_desc", "cursor_string": "opaque+/=&?"}):
        error(admin, query, 503, "official_api_not_configured")
    # The public preview is anchored to a retained map, never an arbitrary
    # upstream set. Unknown identities cannot spend official API quota.
    for identifier in (1, 1999999999):
        result = anonymous.json("GET", f"/api/v1/beatmaps/{identifier}/set/preview", expected=404)
        assert result["error"]["code"] == "not_found"
    assert anonymous.json("GET", "/api/v1/beatmaps?page_size=1") == before
    checks = ["authentication precedes query parsing", "separate import permission",
              "bounded text/cursor and allowlisted filters", "opaque encoded cursor",
              "missing public API configuration is explicit", "retained-map-only public preview",
              "no catalogue publication"]
    print(json.dumps({"status": "passed", "http_requests": Client.request_count,
                      "check_count": len(checks), "checks": checks}))


if __name__ == "__main__":
    run()
