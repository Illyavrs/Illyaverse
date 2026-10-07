"""Final-image official archive refusal checks in a fresh disposable stack.

Cold successful metadata/publication is covered by the original loopback/PG
source tests. This suite deliberately has no official credentials, no upstream
fixture override and no player/game data. It does not certify native gameplay.
"""
import io
import json
import os
import zipfile

from protocol_smoke import Client, multipart


def run():
    assert os.environ.get("ILLYA_ALLOW_DISPOSABLE_TEST") == "1", "Disposable guard required"
    base = os.environ["TEST_SANCTUARY_URL"]
    admin, player, anonymous = (Client(base) for _ in range(3))
    admin.json("POST", "/api/v1/auth/login", {
        "username": os.environ["TEST_ADMIN_USERNAME"], "password": os.environ["TEST_ADMIN_PASSWORD"],
    })
    player.json("POST", "/api/v1/auth/login", {
        "username": os.environ["TEST_PLAYER_USERNAME"], "password": os.environ["TEST_PLAYER_PASSWORD"],
    })
    before = anonymous.json("GET", "/api/v1/beatmaps?page_size=1")["total"]
    assert before == 0, "Fresh disposable catalogue required"
    set_id = 864321
    template = ("osu file format v14\n[General]\nMode:0\n[Metadata]\n"
                "Title:Independent official archive fixture\nArtist:Illyaverse\nCreator:Fixture\n"
                "BeatmapSetID:864321\nBeatmapID:{id}\nVersion:{version}\n"
                "[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\n"
                "SliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n"
                "[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n")
    output = io.BytesIO()
    with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
        for index, version in enumerate(("Primary", "Sibling")):
            archive.writestr(f"{version}.osu", template.format(id=765431 + index, version=version))
        archive.writestr("assets/original.bin", b"original synthetic asset bytes\x00\x01\x02")
    original = output.getvalue()

    def request(client, fields, name, content, status, code=None):
        body, headers = multipart(fields, [("file", name, content)])
        result = client.json("POST", "/api/v1/beatmaps/import/archive", body, headers, expected=status)
        if code is not None:
            assert result["error"]["code"] == code, "Unexpected archive boundary code"
        return result

    fields = [("official_set_id", str(set_id))]
    request(anonymous, fields, "complete.osz", original, 401)
    request(player, fields, "complete.osz", original, 403)
    request(admin, [], "complete.osz", original, 400, "official_archive_invalid")
    request(admin, fields, "complete.txt", original, 400, "official_archive_invalid")
    request(admin, fields, "complete.osz", b"invalid synthetic archive", 400, "official_archive_invalid")
    request(admin, [("official_set_id", "2000000000")], "complete.osz", original,
            409, "official_namespace_conflict")
    request(admin, [("official_set_id", str(set_id + 1))], "complete.osz", original,
            409, "official_archive_manifest_mismatch")
    request(admin, fields, "complete.osz", original, 503, "official_api_not_configured")
    assert anonymous.json("GET", "/api/v1/beatmaps?page_size=1")["total"] == before
    checks = ["authentication and separate import permission", "multipart/extension/archive structure refusals",
              "managed official namespace", "embedded set identity", "unconfigured upstream and no partial catalogue"]
    print(json.dumps({"status": "passed", "http_requests": Client.request_count,
                      "check_count": len(checks), "checks": checks}))


if __name__ == "__main__":
    run()
