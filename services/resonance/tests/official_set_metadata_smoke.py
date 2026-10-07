"""Final-image official set metadata boundaries in a fresh disposable stack.

This suite has no official credentials or upstream override. Successful cold
metadata discovery is covered by the original loopback/PG source fixtures.
No beatmaps, scores, archives or replay files are uploaded by this suite, and
passing it does not certify native gameplay or successful cold discovery.
"""
import json
import os

from protocol_smoke import Client


def run():
    assert os.environ.get("ILLYA_ALLOW_DISPOSABLE_TEST") == "1", "Disposable guard required"
    site, anonymous = (Client(os.environ["TEST_SANCTUARY_URL"]) for _ in range(2))
    gateway = Client(os.environ["TEST_RESONANCE_URL"])
    site.json("POST", "/api/v1/auth/login", {
        "username": os.environ["TEST_ADMIN_USERNAME"],
        "password": os.environ["TEST_ADMIN_PASSWORD"],
    })
    oauth = gateway.json("POST", "/oauth/token", {
        "grant_type": "password", "username": os.environ["TEST_PLAYER_USERNAME"],
        "password": os.environ["TEST_PLAYER_PASSWORD"],
    })
    bearer = {"Authorization": "Bearer " + oauth["access_token"]}
    catalogue_before = anonymous.json("GET", "/api/v1/beatmaps?page_size=1")
    statistics_before = gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"]
    assert catalogue_before["total"] == 0, "Fresh disposable catalogue required"
    assert all(statistics_before[key] == 0 for key in ("play_count", "ranked_score", "pp")), \
        "Fresh disposable player required"

    def native_error(path, status, code, authenticated=True):
        response = gateway.json("GET", path, headers=bearer if authenticated else None,
                                expected=status)
        assert response == {"error": code, "message": code}, "Unexpected native boundary code"

    for path in ("/api/v2/beatmapsets/864321", "/api/v2/beatmapsets/0",
                 "/api/v2/beatmapsets/lookup?beatmap_id=1"):
        native_error(path, 401, "missing_access_token", authenticated=False)

    # Even an administrator's website session is not a service key. Refusal
    # precedes malformed JSON parsing and the 4 KiB metadata body read.
    internal = "/api/v1/internal/beatmapsets/metadata"
    for key in (None, "wrong-fixture-key"):
        headers = {"Content-Type": "application/json"}
        if key is not None:
            headers["x-illya-service-key"] = key
        for body in (b"not JSON", b"x" * (4 * 1024 + 1)):
            response = site.json("POST", internal, body, headers, expected=401)
            assert response["error"]["code"] == "unauthorized", "Service key must precede body work"

    for value in (0, -1, 2147483648):
        native_error(f"/api/v2/beatmapsets/{value}", 400, "invalid_beatmapset_id")
    for query in ("?beatmap_id=0", "?beatmap_id=-1", "?beatmap_id=2147483648", ""):
        native_error("/api/v2/beatmapsets/lookup" + query, 400, "invalid_beatmap_id")

    # These unknown IDs belong to the local namespace. An unconfigured official
    # service would return 503 if used; local lookups must instead remain 404.
    for value in (2000000000, 2147483647):
        native_error(f"/api/v2/beatmapsets/{value}", 404, "beatmapset_not_found")
        native_error(f"/api/v2/beatmapsets/lookup?beatmap_id={value}", 404, "beatmap_not_found")

    # Both legal official-ID edges and a repeated ordinary set use the fixed
    # internal metadata route, mapping its unavailable response without leakage.
    for value in (1, 864321, 1999999999, 864321):
        native_error(f"/api/v2/beatmapsets/{value}", 503, "set_metadata_unavailable")
    for value in (864321, 2000000000):
        native_error(f"/api/v2/beatmapsets/{value}/download", 404, "archive_not_available")

    assert anonymous.json("GET", "/api/v1/beatmaps?page_size=1") == catalogue_before
    assert gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"] == statistics_before
    checks = ["native authentication before set discovery",
              "internal service key before malformed and oversized body parsing",
              "native set and lookup ID bounds",
              "local namespace lookup remains local",
              "unconfigured official set metadata maps to bounded native errors",
              "metadata does not publish content, archives or player statistics"]
    assert Client.request_count == 30, "Keep final-image request accounting exact"
    print(json.dumps({"status": "passed", "http_requests": Client.request_count,
                      "check_count": len(checks), "checks": checks}))


if __name__ == "__main__":
    run()
