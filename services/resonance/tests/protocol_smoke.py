"""Exercise live Resonance + Sanctuary against an explicitly disposable local DB.

Install tests/requirements.txt, start both services against the same disposable DB,
and provide TEST_ADMIN_USERNAME/PASSWORD and TEST_PLAYER_USERNAME/PASSWORD.
This creates one local beatmap, submits scores, and moderates the test scores.
It never modifies osu! clients, hosts files, certificates, or a remote service.
"""
import base64
import datetime
import hashlib
import http.cookiejar
import json
import io
import lzma
import os
import re
import struct
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile

from py3rijndael import Pkcs7Padding, RijndaelCbc


class Client:
    def __init__(self, base):
        parsed = urllib.parse.urlparse(base)
        assert parsed.hostname in ("127.0.0.1", "localhost"), "Disposable localhost only"
        self.base = base.rstrip("/")
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def request(self, method, path, body=None, headers=None, expected=200):
        headers = dict(headers or {})
        if isinstance(body, dict):
            body = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        headers.setdefault("Origin", "http://localhost:3000")
        req = urllib.request.Request(self.base + path, data=body, headers=headers, method=method)
        try:
            response = self.opener.open(req, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        data = response.read()
        assert response.status == expected, (method, path.split("?")[0], response.status, data[:1000])
        return data, response.headers

    def json(self, method, path, body=None, headers=None, expected=200):
        return json.loads(self.request(method, path, body, headers, expected)[0])


def multipart(fields, files=()):
    boundary = "Illyaverse-" + uuid.uuid4().hex
    body = bytearray()
    for name, value in fields:
        body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"\r\n\r\n{value}\r\n'.encode())
    for name, filename, value in files:
        body.extend(f'--{boundary}\r\nContent-Disposition: form-data; name="{name}"; filename="{filename}"\r\nContent-Type: application/octet-stream\r\n\r\n'.encode())
        body.extend(value)
        body.extend(b"\r\n")
    body.extend(f"--{boundary}--\r\n".encode())
    return bytes(body), {"Content-Type": f"multipart/form-data; boundary={boundary}"}


def packets(data):
    output = []
    cursor = 0
    while cursor < len(data):
        packet_id, reserved, size = struct.unpack_from("<HBI", data, cursor)
        assert reserved == 0 and cursor + 7 + size <= len(data)
        output.append((packet_id, data[cursor + 7:cursor + 7 + size]))
        cursor += 7 + size
    assert cursor == len(data)
    return output


def wire_string(text):
    raw = text.encode()
    size = len(raw)
    encoded = bytearray(b"\x0b")
    while size >= 128:
        encoded.append((size & 127) | 128)
        size >>= 7
    encoded.append(size)
    return bytes(encoded) + raw


def packet(packet_id, payload=b""):
    return struct.pack("<HBI", packet_id, 0, len(payload)) + payload


def digest(text):
    return hashlib.md5(text.encode()).hexdigest()


def room_payload(host_id, map_id, map_hash, password):
    body = struct.pack("<HBBI", 0, 0, 0, 0)
    body += wire_string("Independent fixture room") + wire_string(password)
    body += wire_string("Illyaverse fixture") + struct.pack("<i", map_id) + wire_string(map_hash)
    body += bytes([4] + [1] * 15) + bytes(16) + struct.pack("<i", host_id)
    body += struct.pack("<iBBBBi", host_id, 0, 0, 0, 0, 12345)
    return body


def run():
    assert os.environ.get("ILLYA_ALLOW_DISPOSABLE_TEST") == "1", "Set ILLYA_ALLOW_DISPOSABLE_TEST=1 for the disposable test environment"
    admin_name, admin_password = os.environ["TEST_ADMIN_USERNAME"], os.environ["TEST_ADMIN_PASSWORD"]
    player_name, player_password = os.environ["TEST_PLAYER_USERNAME"], os.environ["TEST_PLAYER_PASSWORD"]
    admin = Client(os.environ.get("TEST_SANCTUARY_URL", "http://127.0.0.1:18080"))
    gateway = Client(os.environ.get("TEST_RESONANCE_URL", "http://127.0.0.1:18081"))
    admin.json("POST", "/api/v1/auth/login", {"username": admin_name, "password": admin_password})

    fixture = ("osu file format v14\n[General]\nMode:0\n[Metadata]\nTitle:Protocol fixture " + uuid.uuid4().hex + "\nArtist:Illyaverse\nCreator:Test\nVersion:Easy\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:5\nApproachRate:5\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,500,4,2,1,100,1,0\n[HitObjects]\n64,64,1000,1,0,0:0:0:0:\n256,192,2000,1,0,0:0:0:0:\n").encode()
    archive = io.BytesIO()
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as package:
        package.writestr("protocol.osu", fixture)
    archive = archive.getvalue()
    body, headers = multipart([], [("file", "protocol.osz", archive)])
    beatmap = admin.json("POST", "/api/v1/beatmaps/upload", body, headers)["items"][0]
    map_id, map_hash = beatmap["id"], beatmap["md5"]
    assert gateway.request("GET", f"/osu/{map_id}")[0] == fixture

    version = "20261007"
    client_hash = digest("fixture-path") + ":adapter:" + digest("fixture-adapter") + ":" + digest("fixture-uninstall") + ":" + digest("fixture-disk") + ":"
    def login(name, password):
        data, headers = gateway.request("POST", "/", f"{name}\n{digest(password)}\nb{version}|8|0|{client_hash}|0\n".encode())
        userid = next(struct.unpack("<i", p)[0] for i, p in packets(data) if i == 5)
        assert userid > 0
        assert headers["cho-token"] != "no"
        return userid, {"osu-token": headers["cho-token"]}

    player_id, player_bancho = login(player_name, player_password)
    admin_id, admin_bancho = login(admin_name, admin_password)
    response = gateway.request("POST", "/", packet(4), player_bancho)[0]
    assert any(i == 8 for i, _ in packets(response)), "Ping/pong failed"
    text = "Protocol smoke " + uuid.uuid4().hex
    chat = wire_string("spoofed-admin") + wire_string(text) + wire_string("#general") + struct.pack("<i", 9999)
    gateway.request("POST", "/", packet(1, chat), player_bancho)
    received = gateway.request("POST", "/", packet(4), admin_bancho)[0]
    assert any(i == 7 and text.encode() in p and player_name.encode() in p and b"spoofed-admin" not in p for i, p in packets(received)), "Chat sender binding or broadcast failed"

    oauth = gateway.json("POST", "/oauth/token", {"grant_type": "password", "username": player_name, "password": player_password})
    bearer = {"Authorization": "Bearer " + oauth["access_token"]}
    assert gateway.json("GET", "/api/v2/me", headers=bearer)["id"] == player_id
    lookup = gateway.json("GET", "/api/v2/beatmaps/lookup?checksum=" + map_hash, headers=bearer)
    assert lookup["id"] == map_id and lookup["checksum"] == map_hash
    assert gateway.json("GET", f"/api/v2/beatmapsets/lookup?beatmap_id={map_id}", headers=bearer)["beatmaps"][0]["checksum"] == map_hash
    assert any(s["id"] == map_id for s in gateway.json("GET", "/api/v2/beatmapsets/search?q=Protocol%20fixture", headers=bearer)["beatmapsets"])
    assert gateway.request("GET", f"/api/v2/beatmapsets/{map_id}/download", headers=bearer)[0] == archive
    channels = gateway.json("GET", "/api/v2/chat/channels", headers=bearer)
    assert all(c["name"] != "#staff" for c in channels), "Staff channel leaked to player"
    gateway.request("POST", "/", packet(73, struct.pack("<i", admin_id)), player_bancho)
    assert any(r["target_id"] == admin_id for r in gateway.json("GET", "/api/v2/friends", headers=bearer))
    gateway.request("POST", "/", packet(74, struct.pack("<i", admin_id)), player_bancho)
    assert not gateway.json("GET", "/api/v2/friends", headers=bearer)
    gateway.json("POST", f"/api/v2/friends?target={admin_id}", headers=bearer)
    gateway.request("POST", f"/api/v2/friends?target={player_id}", headers=bearer, expected=400)
    admin_oauth = gateway.json("POST", "/oauth/token", {"grant_type": "password", "username": admin_name, "password": admin_password})
    admin_bearer = {"Authorization": "Bearer " + admin_oauth["access_token"]}
    gateway.json("POST", f"/api/v2/blocks?target={player_id}", headers=admin_bearer)
    assert not gateway.json("GET", "/api/v2/friends", headers=bearer)
    gateway.request("POST", f"/api/v2/friends?target={admin_id}", headers=bearer, expected=403)
    direct = wire_string("spoof") + wire_string("Blocked fixture") + wire_string(admin_name) + struct.pack("<i", 9999)
    blocked = gateway.request("POST", "/", packet(25, direct), player_bancho)[0]
    assert any(i == 100 for i, _ in packets(blocked)), "Block failed to prevent direct message"
    gateway.request("DELETE", f"/api/v2/blocks/{player_id}", headers=admin_bearer, expected=204)
    gateway.request("POST", "/", packet(99, struct.pack("<i", 1)), admin_bancho)
    assert any(i == 100 for i, _ in packets(gateway.request("POST", "/", packet(25, direct), player_bancho)[0]))
    gateway.request("POST", "/", packet(73, struct.pack("<i", player_id)), admin_bancho)
    gateway.request("POST", "/", packet(25, direct), player_bancho)
    assert any(i == 7 and b"Blocked fixture" in p for i, p in packets(gateway.request("POST", "/", packet(4), admin_bancho)[0])), "Friend-only direct message setting rejected a friend"
    gateway.request("POST", "/", packet(99, struct.pack("<i", 0)), admin_bancho)

    gateway.request("POST", "/", packet(30), admin_bancho)
    created = gateway.request("POST", "/", packet(31, room_payload(player_id, map_id, map_hash, "fixture-room-password")), player_bancho)[0]
    room_id = next(struct.unpack_from("<H", p)[0] for i, p in packets(created) if i == 36)
    assert b"fixture-room-password" not in created, "Room password leaked into a response"
    bad_join = gateway.request("POST", "/", packet(32, struct.pack("<i", room_id) + wire_string("wrong")), admin_bancho)[0]
    assert any(i == 37 for i, _ in packets(bad_join))
    joined = gateway.request("POST", "/", packet(32, struct.pack("<i", room_id) + wire_string("fixture-room-password")), admin_bancho)[0]
    assert any(i == 36 for i, _ in packets(joined))
    gateway.request("POST", "/", packet(44), admin_bancho, expected=403)
    gateway.request("POST", "/", packet(39), player_bancho)
    gateway.request("POST", "/", packet(39), admin_bancho)
    gateway.request("POST", "/", packet(44), player_bancho)
    assert any(i == 46 for i, _ in packets(gateway.request("POST", "/", packet(4), admin_bancho)[0]))
    frame = struct.pack("<iBHHHHHHiHH?BB?", 1000, 15, 2, 0, 0, 0, 0, 0, 12345, 2, 2, True, 100, 0, False)
    gateway.request("POST", "/", packet(47, frame), player_bancho)
    assert any(i == 48 and p[4] == 0 for i, p in packets(gateway.request("POST", "/", packet(4), admin_bancho)[0])), "Score frame used a forged slot identity"
    gateway.request("POST", "/", packet(52), player_bancho)
    gateway.request("POST", "/", packet(52), admin_bancho)
    assert any(i == 53 for i, _ in packets(gateway.request("POST", "/", packet(4), player_bancho)[0]))
    gateway.request("POST", "/", packet(49), player_bancho)
    completed = gateway.request("POST", "/", packet(49), admin_bancho)[0]
    assert any(i == 58 for i, _ in packets(completed))
    gateway.request("POST", "/", packet(70, struct.pack("<i", 1)), player_bancho)
    assert any(i == 50 for i, _ in packets(gateway.request("POST", "/", packet(4), admin_bancho)[0]))
    gateway.request("POST", "/", packet(33), admin_bancho)
    gateway.request("POST", "/", packet(33), player_bancho)
    gateway.request("POST", "/", packet(16, struct.pack("<i", player_id)), admin_bancho)
    assert any(i == 13 for i, _ in packets(gateway.request("POST", "/", packet(4), player_bancho)[0]))
    spectator_bundle = struct.pack("<iHB", 0, 0, 0) + frame + struct.pack("<H", 1)
    gateway.request("POST", "/", packet(18, spectator_bundle), player_bancho)
    assert any(i == 15 and p == spectator_bundle for i, p in packets(gateway.request("POST", "/", packet(4), admin_bancho)[0]))
    gateway.request("POST", "/", packet(17), admin_bancho)
    assert any(i == 14 for i, _ in packets(gateway.request("POST", "/", packet(4), player_bancho)[0]))
    token_fields = urllib.parse.urlencode({"beatmap_hash": map_hash, "version_hash": "independent-smoke-fixture", "ruleset_id": 0}).encode()
    token = gateway.json("POST", f"/api/v2/beatmaps/{map_id}/solo/scores", token_fields, dict(bearer, **{"Content-Type": "application/x-www-form-urlencoded"}))["id"]
    score = {"ruleset_id": 0, "passed": True, "total_score": 1000000, "total_score_without_mods": 1000000, "accuracy": 1, "max_combo": 2, "rank": "X", "mods": [], "statistics": {"great": 2}, "maximum_statistics": {"great": 2}, "pauses": [], "user_id": 9999, "beatmap_id": 9999}
    path = f"/api/v2/beatmaps/{map_id}/solo/scores/{token}"
    lazer = gateway.json("PUT", path, score, bearer)
    assert lazer["user_id"] == player_id and lazer["beatmap_id"] == map_id and lazer["verification"] == "pending" and lazer["pp"] > 0
    assert gateway.json("PUT", path, score, bearer)["id"] == lazer["id"], "Score retry did not deduplicate"
    gateway.request("PUT", path, dict(score, total_score=999999), bearer, expected=409)

    played_at = datetime.datetime.now(datetime.timezone.utc).strftime("%y%m%d%H%M%S")
    stable_raw = 600  # Two Great circles, no legacy combo bonus.
    checksum_text = f"chickenmcnuggets2o1500smustard00uu{map_hash}2True{player_name}{stable_raw}X0QTrue0{version}{played_at}{client_hash}"
    checksum = digest(checksum_text)
    envelope = f"{map_hash}:{player_name}:{checksum}:2:0:0:0:0:0:{stable_raw}:2:True:X:0:True:0:{played_at}:b{version}"
    iv = bytes(range(32))
    cipher = RijndaelCbc(("osu!-scoreburgr---------" + version).encode(), iv, Pkcs7Padding(32), block_size=32)
    encrypt = lambda value: base64.b64encode(cipher.encrypt(value.encode())).decode()
    frames = lzma.compress(b"1000|64|64|1,1000|256|192|1,-12345|0|0|12345,", format=lzma.FORMAT_ALONE)
    fields = [("score", encrypt(envelope)), ("iv", base64.b64encode(iv).decode()), ("osuver", version), ("pass", digest(player_password)), ("bmk", map_hash), ("sbk", ""), ("s", encrypt(client_hash))]
    body, headers = multipart(fields, [("score", "replay.osr", frames)])
    stable = gateway.request("POST", "/web/osu-submit-modular-selector.php", body, headers)[0].decode()
    stable_id = int(re.search(r"onlineScoreId:(\d+)", stable).group(1))
    again = gateway.request("POST", "/web/osu-submit-modular-selector.php", body, headers)[0].decode()
    assert f"onlineScoreId:{stable_id}|" in again
    query = urllib.parse.urlencode({"u": player_name, "h": digest(player_password), "c": stable_id, "m": 0})
    assert gateway.request("GET", "/web/osu-getreplay.php?" + query)[0] == frames
    stable_score = gateway.json("GET", f"/api/v2/scores/{stable_id}", headers=bearer)
    assert stable_score["has_replay"] and stable_score["verification"] == "pending" and stable_score["pp"] > 0
    altered_body, altered_headers = multipart(fields, [("score", "replay.osr", lzma.compress(b"1|0|0|0,", format=lzma.FORMAT_ALONE))])
    gateway.request("POST", "/web/osu-submit-modular-selector.php", altered_body, altered_headers, expected=409)
    invalid_fields = [(name, encrypt(envelope.replace(checksum, "0" * 32)) if name == "score" else value) for name, value in fields]
    invalid_body, invalid_headers = multipart(invalid_fields, [("score", "replay.osr", frames)])
    gateway.request("POST", "/web/osu-submit-modular-selector.php", invalid_body, invalid_headers, expected=400)
    leaderboard_query = urllib.parse.urlencode({"u": player_name, "h": digest(player_password), "c": map_hash, "m": 0})
    score_line = re.compile(rf"^{stable_id}\|{re.escape(player_name)}\|", re.MULTILINE)
    assert not score_line.search(gateway.request("GET", "/web/osu-osz2-getscores.php?" + leaderboard_query)[0].decode()), "Pending score leaked to ranking"
    admin.json("PATCH", f"/api/v1/admin/beatmaps/{map_id}", {"status": "ranked"})
    admin.json("PATCH", f"/api/v1/admin/scores/{stable_id}", {"verification": "verified"})
    assert score_line.search(gateway.request("GET", "/web/osu-osz2-getscores.php?" + leaderboard_query)[0].decode())
    native_scores = gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&type=global", headers=bearer)
    assert native_scores["score_count"] == 1 and native_scores["scores"][0]["id"] == stable_id
    converted = native_scores["scores"][0]
    assert converted["legacy_total_score"] == stable_raw and converted["total_score"] == 985000
    assert converted["score_scale_status"] == "legacy_estimated" and any(m["acronym"] == "CL" for m in converted["mods"])
    assert gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&mods[]=CL", headers=bearer)["scores"][0]["id"] == stable_id
    assert gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&mods[]=NM", headers=bearer)["scores"] == []
    admin.json("PATCH", f"/api/v1/admin/scores/{lazer['id']}", {"verification": "verified"})
    native_scores = gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&type=global", headers=bearer)
    assert native_scores["score_count"] == 1 and native_scores["scores"][0]["id"] == lazer["id"]
    assert native_scores["user_score"]["position"] == 1
    assert native_scores["scores"][0]["rank"] == "X" and native_scores["scores"][0]["statistics"]["great"] == 2
    assert score_line.search(gateway.request("GET", "/web/osu-osz2-getscores.php?" + leaderboard_query)[0].decode())
    stats = gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"]
    assert stats["pp"] > 0 and stats["ranked_score"] == 100130 and stats["total_score"] == stable_raw + 100130 and stats["play_count"] == 2
    assert stats["global_rank"] > 0 and stats["maximum_combo"] == 2 and stats["level"]["current"] >= 2

    # Stable ScoreV2 has a standardized source scale. Its raw board must stay
    # separate from V1, and its lifetime contribution uses Classic display.
    v2_mods = 1 << 29
    v2_raw = 1000000
    v2_checksum = digest(f"chickenmcnuggets2o1500smustard00uu{map_hash}2True{player_name}{v2_raw}X{v2_mods}QTrue0{version}{played_at}{client_hash}")
    v2_envelope = f"{map_hash}:{player_name}:{v2_checksum}:2:0:0:0:0:0:{v2_raw}:2:True:X:{v2_mods}:True:0:{played_at}:b{version}"
    v2_fields = [(name, encrypt(v2_envelope) if name == "score" else value) for name, value in fields]
    v2_body, v2_headers = multipart(v2_fields, [("score", "replay.osr", frames)])
    v2_response = gateway.request("POST", "/web/osu-submit-modular-selector.php", v2_body, v2_headers)[0].decode()
    v2_id = int(re.search(r"onlineScoreId:(\d+)", v2_response).group(1))
    admin.json("PATCH", f"/api/v1/admin/scores/{v2_id}", {"verification": "verified"})
    v2_line = re.compile(rf"^{v2_id}\|{re.escape(player_name)}\|", re.MULTILINE)
    v1_board = gateway.request("GET", "/web/osu-osz2-getscores.php?" + leaderboard_query)[0].decode()
    assert score_line.search(v1_board) and not v2_line.search(v1_board)
    v2_board = gateway.request("GET", "/web/osu-osz2-getscores.php?" + leaderboard_query + f"&mods={v2_mods}")[0].decode()
    assert v2_line.search(v2_board) and not score_line.search(v2_board)
    v2_score = gateway.json("GET", f"/api/v2/scores/{v2_id}", headers=bearer)
    assert v2_score["legacy_total_score"] == v2_raw and v2_score["total_score"] == v2_raw
    assert v2_score["classic_total_score"] == 100130 and v2_score["score_scale_status"] == "legacy_scorev2"
    stats = gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"]
    assert stats["total_score"] == stable_raw + 2 * 100130 and stats["ranked_score"] == 100130

    rotated = gateway.json("POST", "/oauth/token", {"grant_type": "refresh_token", "refresh_token": oauth["refresh_token"]})
    gateway.request("GET", "/api/v2/me", headers=bearer, expected=401)
    assert gateway.json("GET", "/api/v2/me", headers={"Authorization": "Bearer " + rotated["access_token"]})["id"] == player_id
    gateway.request("POST", "/", packet(2), player_bancho)
    after_logout = gateway.request("POST", "/", packet(4), player_bancho)[0]
    assert any(i == 86 for i, _ in packets(after_logout)), "Logged out stable session remained valid"
    print(json.dumps({"status": "passed", "beatmap_id": map_id, "stable_score_id": stable_id, "lazer_score_id": lazer["id"], "checks": ["shared auth", "stable binary login/ping/chat/logout", "lazer OAuth/refresh", "lazer checksum/set lookup/archive/score collections", "Classic profile totals and standardized mixed-client ranking", "stable ScoreV1/ScoreV2 board partition", "RBAC", "friend/block/DM settings", "stable password room/host authority/slot identity", "stable ready/start/load/score/complete/transfer/leave", "stable spectator frame relay", "lazer score token/idempotency", "stable Rijndael256 submission/checksum", "server PP", "replay round trip", "pending rankings", "moderation"]}))


if __name__ == "__main__":
    run()
