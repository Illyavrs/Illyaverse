"""Exercise live Resonance + Sanctuary against an explicitly disposable local DB.

Install tests/requirements.txt, start both services against the same disposable DB,
and provide TEST_ADMIN_USERNAME/PASSWORD and TEST_PLAYER_USERNAME/PASSWORD.
This creates local beatmaps and synthetic players, submits scores, and moderates
the test scores. Native room fixtures use an original standard-library SSE client.
It never modifies osu! clients, hosts files, certificates, or a remote service.
"""
import base64
import contextlib
import datetime
import hashlib
import http.cookiejar
import json
import io
import lzma
import math
import os
import queue
import re
import struct
import threading
import time
import urllib.error
import urllib.parse
import urllib.request
import uuid
import zipfile

from py3rijndael import Pkcs7Padding, RijndaelCbc


class Client:
    request_count = 0

    def __init__(self, base):
        parsed = urllib.parse.urlparse(base)
        assert parsed.hostname in ("127.0.0.1", "localhost"), "Disposable localhost only"
        self.base = base.rstrip("/")
        self.opener = urllib.request.build_opener(urllib.request.HTTPCookieProcessor(http.cookiejar.CookieJar()))

    def request(self, method, path, body=None, headers=None, expected=200, native=False):
        headers = dict(headers or {})
        if isinstance(body, dict):
            body = json.dumps(body).encode()
            headers["Content-Type"] = "application/json"
        if not native:
            headers.setdefault("Origin", "http://localhost:3000")
        req = urllib.request.Request(self.base + path, data=body, headers=headers, method=method)
        Client.request_count += 1
        try:
            response = self.opener.open(req, timeout=30)
        except urllib.error.HTTPError as error:
            response = error
        data = response.read()
        assert response.status == expected, (method, path.split("?")[0], response.status, data[:1000])
        return data, response.headers

    def json(self, method, path, body=None, headers=None, expected=200, native=False):
        return json.loads(self.request(method, path, body, headers, expected, native)[0])


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


def replay_metadata(data):
    """Read the fixture .osr header independently of the server's parser."""
    mode, version = struct.unpack_from("<Bi", data)
    cursor = 5

    def text():
        nonlocal cursor
        marker = data[cursor]
        cursor += 1
        if marker == 0:
            return ""
        assert marker == 11
        size, shift = 0, 0
        while True:
            value = data[cursor]
            cursor += 1
            size |= (value & 127) << shift
            if value < 128:
                break
            shift += 7
            assert shift <= 28
        value = data[cursor:cursor + size].decode()
        cursor += size
        return value

    map_hash, username, checksum = text(), text(), text()
    hits = struct.unpack_from("<6H", data, cursor)
    cursor += 12
    raw, combo, perfect, mods = struct.unpack_from("<iHBI", data, cursor)
    cursor += 11
    text()  # Life bar.
    cursor += 8  # .NET timestamp.
    size = struct.unpack_from("<i", data, cursor)[0]
    cursor += 4
    assert size >= 0 and cursor + size + 8 == len(data)
    frames = data[cursor:cursor + size]
    score_id = struct.unpack_from("<q", data, cursor + size)[0]
    return {"mode": mode, "version": version, "map_hash": map_hash,
            "username": username, "checksum": checksum, "hits": hits,
            "raw": raw, "combo": combo, "perfect": perfect, "mods": mods,
            "frames": frames, "score_id": score_id}


def room_payload(host_id, map_id, map_hash, password):
    body = struct.pack("<HBBI", 0, 0, 0, 0)
    body += wire_string("Independent fixture room") + wire_string(password)
    body += wire_string("Illyaverse fixture") + struct.pack("<i", map_id) + wire_string(map_hash)
    body += bytes([4] + [1] * 15) + bytes(16) + struct.pack("<i", host_id)
    body += struct.pack("<iBBBBi", host_id, 0, 0, 0, 0, 12345)
    return body


class SignalRSSE:
    """Independent JSON SignalR fixture using ordinary loopback HTTP + SSE."""

    def __init__(self, gateway, auth, hub):
        assert hub in ("multiplayer", "metadata", "spectator")
        self.gateway, self.auth = gateway, dict(auth)
        self.messages, self.saved = queue.Queue(maxsize=256), []
        self.counter, self.closed = 0, False
        route = "/signalr/" + hub
        negotiated = gateway.json("POST", route + "/negotiate?negotiateVersion=1",
                                  headers=auth, native=True)
        assert any(item["transport"] == "ServerSentEvents" for item in negotiated["availableTransports"])
        self.path = route + "?" + urllib.parse.urlencode({"id": negotiated["connectionToken"]})
        request = urllib.request.Request(gateway.base + self.path,
                                         headers=dict(auth, Accept="text/event-stream"))
        Client.request_count += 1
        self.response = urllib.request.urlopen(request, timeout=15)
        assert self.response.status == 200
        assert self.response.headers["Content-Type"].startswith("text/event-stream")
        self.reader = threading.Thread(target=self._read, daemon=True)
        self.reader.start()
        try:
            self._send({"protocol": "json", "version": 1})
            self._wait(lambda message: message == {})
        except BaseException:
            self.close()
            raise

    def _read(self):
        try:
            fields = []
            while not self.closed:
                line = self.response.readline(1_048_577)
                if not line:
                    raise AssertionError("Fixture SSE connection ended")
                assert len(line) <= 1_048_576, "Oversized fixture SSE event"
                line = line.rstrip(b"\r\n")
                if line.startswith(b"data:"):
                    fields.append(line[5:].lstrip(b" "))
                    assert sum(map(len, fields)) <= 1_048_576
                elif not line and fields:
                    payload, fields = b"\n".join(fields), []
                    assert payload.endswith(b"\x1e"), "Missing SignalR record separator"
                    for record in payload[:-1].split(b"\x1e"):
                        message = json.loads(record)
                        if message.get("type") != 6:
                            self.messages.put(message, timeout=2)
                        if message.get("type") == 7:
                            return
        except BaseException as error:
            if not self.closed:
                self.messages.put(error, timeout=2)

    def _send(self, message):
        body = (json.dumps(message, separators=(",", ":")) + "\x1e").encode()
        self.gateway.request("POST", self.path, body,
                             dict(self.auth, **{"Content-Type": "application/json"}), native=True)

    def _wait(self, predicate, timeout=10):
        for index, message in enumerate(self.saved):
            if predicate(message):
                return self.saved.pop(index)
        deadline = time.monotonic() + timeout
        while True:
            remaining = deadline - time.monotonic()
            assert remaining > 0, "Timed out waiting for fixture SignalR message"
            try:
                message = self.messages.get(timeout=remaining)
            except queue.Empty:
                raise AssertionError("Timed out waiting for fixture SignalR message") from None
            if isinstance(message, BaseException):
                raise message
            assert message.get("type") != 7, "Fixture SignalR connection closed"
            if predicate(message):
                return message
            self.saved.append(message)
            assert len(self.saved) <= 256, "Too many unmatched fixture hub messages"

    def call(self, target, arguments=(), error=None):
        self.counter += 1
        identity = str(self.counter)
        self._send({"type": 1, "invocationId": identity,
                    "target": target, "arguments": list(arguments)})
        result = self._wait(lambda message: message.get("type") == 3 and message.get("invocationId") == identity)
        if error is not None:
            assert result.get("error") == error, (target, result)
        else:
            assert "error" not in result, (target, result)
        return result.get("result")

    def event(self, target):
        return self._wait(lambda message: message.get("type") == 1 and message.get("target") == target)["arguments"]

    def no_event(self, target, timeout=0.15):
        predicate = lambda message: message.get("type") == 1 and message.get("target") == target
        assert not any(predicate(message) for message in self.saved), target
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                message = self.messages.get(timeout=max(0.001, deadline - time.monotonic()))
            except queue.Empty:
                return
            if isinstance(message, BaseException):
                raise message
            assert not predicate(message), (target, message)
            self.saved.append(message)

    def close(self):
        if self.closed:
            return
        self.closed = True
        try:
            self.gateway.request("DELETE", self.path, headers=self.auth, expected=202, native=True)
        finally:
            self.reader.join(timeout=2)
            if not self.reader.is_alive():
                self.response.close()

    def __enter__(self):
        return self

    def __exit__(self, kind, value, traceback):
        if kind is None:
            self.close()
        else:
            try:
                self.close()
            except Exception:
                pass  # Preserve the failing assertion; launcher teardown remains authoritative.


def native_room_smoke(admin, gateway, bearer, player_id, map_id, map_hash, objects):
    """Actual HeadToHead room APIs and Sanctuary calculation, with new fixture peers."""
    initial_requests = Client.request_count
    count = len(objects)

    def new_player(label):
        name = "room_" + label + "_" + uuid.uuid4().hex[:10]
        password = uuid.uuid4().hex + uuid.uuid4().hex
        website = Client(admin.base)
        registered = website.json("POST", "/api/v1/auth/register",
                                  {"username": name, "password": password, "email": name + "@example.test"})["user"]
        assert not any(permission in registered["permissions"] for permission in ("*", "scores.moderate"))
        auth = gateway.json("POST", "/oauth/token",
                            {"grant_type": "password", "username": name, "password": password}, native=True)
        return registered["id"], {"Authorization": "Bearer " + auth["access_token"]}

    peer_id, peer_auth = new_player("peer")
    _, outsider_auth = new_player("outsider")
    settings = {"name": "Independent native room " + uuid.uuid4().hex[:8], "password": "",
                "playlistItemId": 0, "matchType": 1, "queueMode": 0,
                "autoStartDuration": 0, "autoSkip": False, "maxParticipants": 16}
    item = {"beatmapID": map_id, "beatmapChecksum": map_hash, "rulesetID": 0,
            "requiredMods": [], "allowedMods": [], "freestyle": False}
    rx_item = dict(item, allowedMods=[{"acronym": "RX"}])
    token_input = {"beatmap_id": map_id, "beatmap_hash": map_hash,
                   "version_hash": "independent-room-smoke", "ruleset_id": 0}

    def native_score(total, mods=(), weak=False):
        return {"ruleset_id": 0, "passed": True, "total_score": total,
                "total_score_without_mods": 1000000 if mods else total,
                "accuracy": 5 / 6 if weak else 1, "max_combo": count,
                "rank": "B" if weak else "X", "mods": list(mods),
                "statistics": {"great": count - 20, "ok": 20} if weak else {"great": count},
                "maximum_statistics": {"great": count}, "pauses": []}

    with contextlib.ExitStack() as resources:
        host = resources.enter_context(SignalRSSE(gateway, bearer, "multiplayer"))
        peer = resources.enter_context(SignalRSSE(gateway, peer_auth, "multiplayer"))
        outside = resources.enter_context(SignalRSSE(gateway, outsider_auth, "multiplayer"))
        metadata = resources.enter_context(SignalRSSE(gateway, peer_auth, "metadata"))
        outside_metadata = resources.enter_context(SignalRSSE(gateway, outsider_auth, "metadata"))
        spectator = resources.enter_context(SignalRSSE(gateway, bearer, "spectator"))
        other_spectator = resources.enter_context(SignalRSSE(gateway, bearer, "spectator"))
        created = host.call("CreateRoom", [{"settings": settings, "playlist": [item, rx_item]}])
        room = created["roomID"]
        assert created["settings"]["matchType"] == 1
        first_item, second_item = [entry["id"] for entry in created["playlist"]]
        assert first_item < second_item
        peer.call("JoinRoom", [room])
        base = f"/api/v2/rooms/{room}"
        first = base + f"/playlist/{first_item}/scores"
        second = base + f"/playlist/{second_item}/scores"
        outside_metadata.call("BeginWatchingMultiplayerRoom", [room], error="room_membership_required")
        initial_stats = metadata.call("BeginWatchingMultiplayerRoom", [room])
        assert len(initial_stats) == 2
        assert all(entry["totalScoreDistribution"] == [0] * 13 and entry["cumulativeScore"] == 0 for entry in initial_stats)
        gateway.request("GET", first, headers=outsider_auth, expected=404)
        gateway.request("POST", first, token_input, bearer, expected=400)

        def begin_round(route):
            for connection in (host, peer):
                connection.call("ChangeBeatmapAvailability", [[4, None]])
                connection.call("ChangeState", [1])
            host.call("StartMatch")
            assert host.event("LoadRequested") == peer.event("LoadRequested") == []
            host_token = gateway.json("POST", route, token_input, bearer)["id"]
            peer_token = gateway.json("POST", route, token_input, peer_auth)["id"]
            assert host_token != peer_token
            assert gateway.json("POST", route, token_input, bearer)["id"] == host_token
            for connection in (host, peer):
                connection.call("ChangeState", [3])
                connection.call("ChangeState", [4])
            assert host.event("GameplayStarted") == peer.event("GameplayStarted") == []
            return host_token, peer_token

        def finish_round():
            host.call("ChangeState", [6])
            peer.call("ChangeState", [6])
            assert host.event("ResultsReady") == peer.event("ResultsReady") == []

        def score_event(item_id, score, user, total, rank):
            arguments = metadata.event("MultiplayerRoomScoreSet")
            assert len(arguments) == 1
            event = arguments[0]
            assert set(event) == {"roomID", "playlistItemID", "scoreID", "userID", "totalScore", "newRank"}
            assert event == {"roomID": room, "playlistItemID": item_id, "scoreID": score,
                             "userID": user, "totalScore": total, "newRank": rank}

        def accepted(route, token, payload, auth):
            result = gateway.json("PUT", route + f"/{token}", payload, auth)
            assert result["processed"] and not result["ranked"] and result["verification"] == "pending"
            assert result["total_score"] == payload["total_score"] and result["pp"] > 0
            stored = admin.json("GET", f"/api/v1/scores/{result['id']}")
            assert stored["client"] == "lazer" and stored["id"] == result["id"]
            assert stored["native_submission"]["total_score"] == payload["total_score"]
            assert stored["verification"] == "pending", (result["id"], stored["verification"])
            # Admin JSONB decoding can round the same float8 one step from the native binary read.
            assert math.isfinite(stored["pp"]) and math.isfinite(result["pp"])
            assert abs(stored["pp"] - result["pp"]) <= max(math.ulp(stored["pp"]), math.ulp(result["pp"])), {
                "id": result["id"], "mods": payload["mods"],
                "stored_pp": stored["pp"], "result_pp": result["pp"],
                "stored_pp_hex": float(stored["pp"]).hex(),
                "result_pp_hex": float(result["pp"]).hex(),
            }
            return result

        host_token, peer_token = begin_round(first)
        summary = gateway.json("GET", base, headers=bearer)["current_user_score"]
        assert summary["playlist_item_attempts"] == [{"id": first_item, "attempts": 1, "passed": False}]
        nm_input, peer_input = native_score(1000000), native_score(900000, weak=True)
        gateway.request("POST", second, token_input, bearer, expected=400)
        gateway.request("PUT", first + f"/{host_token}", nm_input, peer_auth, expected=404)
        gateway.request("PUT", second + f"/{host_token}", nm_input, bearer, expected=404)
        gateway.request("PUT", first + f"/{host_token}", dict(nm_input, mods=[{"acronym": "RX"}]), bearer, expected=400)
        solo_fields = {key: token_input[key] for key in ("beatmap_hash", "version_hash", "ruleset_id")}
        solo_token = gateway.json("POST", f"/api/v2/beatmaps/{map_id}/solo/scores", solo_fields, bearer)["id"]
        gateway.request("PUT", first + f"/{solo_token}", nm_input, bearer, expected=404)
        gateway.request("PUT", f"/api/v2/beatmaps/{map_id}/solo/scores/{host_token}", nm_input, bearer, expected=404)
        spectator.call("BeginPlaySessionV2", [host_token, [map_id, 0, [], 1, {"5": count}]])
        header = [1000000, 1.0, count, count, {"5": count}, [600.0, 600.0, count, 0.5, 0.0], None, [], None, None]
        spectator.call("SendFrameDataV2", [host_token, [header, [[float(at), float(x), float(y), 1] for x, y, at in objects]]])
        finish_round()  # Lifecycle completion is observable before either score is processed.
        assert gateway.json("GET", first, headers=peer_auth)["total"] == 0
        host_nm = accepted(first, host_token, nm_input, bearer)
        score_event(first_item, host_nm["id"], player_id, 1000000, 1)
        spectator.no_event("UserScoreProcessed")
        spectator.call("EndPlaySessionV2", [host_token, 3])
        assert spectator.event("UserScoreProcessed") == [player_id, host_nm["id"]]
        other_spectator.no_event("UserScoreProcessed")
        assert gateway.json("GET", first + f"/{host_nm['id']}", headers=bearer)["has_replay"]
        assert gateway.json("PUT", first + f"/{host_token}", nm_input, bearer)["id"] == host_nm["id"]
        gateway.request("PUT", first + f"/{host_token}", dict(nm_input, total_score=999999), bearer, expected=409)
        metadata.no_event("MultiplayerRoomScoreSet")
        peer_nm = accepted(first, peer_token, peer_input, peer_auth)
        score_event(first_item, peer_nm["id"], peer_id, 900000, 2)
        pending = gateway.json("GET", first + f"/{host_nm['id']}", headers=peer_auth)
        assert pending["verification"] == "pending" and pending["processed"] and not pending["ranked"]
        gateway.request("GET", f"/api/v2/scores/{host_nm['id']}", headers=peer_auth, expected=404)
        gateway.request("GET", f"/api/v2/scores/{host_nm['id']}/download", headers=peer_auth, expected=404)
        gateway.request("GET", first + f"/{host_nm['id']}", headers=outsider_auth, expected=404)
        gateway.request("GET", second + f"/{host_nm['id']}", headers=peer_auth, expected=404)
        indexed = gateway.json("GET", first + "?limit=1", headers=peer_auth)
        assert indexed["total"] == 2 and [entry["id"] for entry in indexed["scores"]] == [host_nm["id"]]
        assert indexed["user_score"]["id"] == peer_nm["id"] and indexed["cursor"] is not None
        next_params = dict(indexed["params"])
        next_params.update({f"cursor[{key}]": value for key, value in indexed["cursor"].items()})
        next_page = gateway.json("GET", first + "?" + urllib.parse.urlencode(next_params), headers=peer_auth)
        assert [entry["id"] for entry in next_page["scores"]] == [peer_nm["id"]] and next_page["cursor"] is None
        user_score = gateway.json("GET", first + f"/users/{peer_id}", headers=peer_auth)
        assert user_score["id"] == peer_nm["id"] and user_score["position"] == 2
        assert [entry["id"] for entry in user_score["scores_around"]["higher"]["scores"]] == [host_nm["id"]]
        host.call("ChangeUserMods", [[{"acronym": "RX"}]])
        rx_token, peer_second_token = begin_round(second)
        rx_input, peer_second_input = native_score(0, [{"acronym": "RX"}]), native_score(1000000)
        gateway.request("PUT", second + f"/{rx_token}", dict(rx_input, mods=[]), bearer, expected=400)
        finish_round()
        room_rx = accepted(second, rx_token, rx_input, bearer)
        assert room_rx["score_variant"] == "relax" and room_rx["score_scale_status"] == "unavailable"
        assert room_rx["classic_total_score"] is None and room_rx["mods"] == [{"acronym": "RX"}]
        score_event(second_item, room_rx["id"], player_id, 0, 1)
        assert gateway.json("PUT", second + f"/{rx_token}", rx_input, bearer)["id"] == room_rx["id"]
        gateway.request("PUT", second + f"/{rx_token}", dict(rx_input, total_score=1), bearer, expected=409)
        peer_second = accepted(second, peer_second_token, peer_second_input, peer_auth)
        assert peer_second["score_variant"] == "vanilla" and peer_second["mods"] == []
        score_event(second_item, peer_second["id"], peer_id, 1000000, 1)
        rx_pending = gateway.json("GET", second + f"/users/{player_id}", headers=peer_auth)
        assert rx_pending["id"] == room_rx["id"] and rx_pending["position"] == 2 and rx_pending["total_score"] == 0
        gateway.request("GET", f"/api/v2/scores/{room_rx['id']}", headers=peer_auth, expected=404)
        board = gateway.json("GET", base + "/leaderboard", headers=peer_auth)
        assert [entry["user_id"] for entry in board["leaderboard"]] == [peer_id, player_id]
        assert [entry["total_score"] for entry in board["leaderboard"]] == [1900000, 1000000]
        assert all(entry["completed"] == 2 and entry["attempts"] == 2 and entry["pp"] is None for entry in board["leaderboard"])
        assert board["user_score"]["user_id"] == peer_id and board["user_score"]["position"] == 1
        summary = gateway.json("GET", base, headers=bearer)["current_user_score"]
        assert summary["total_score"] == 1000000 and summary["completed"] == 2 and summary["position"] == 2
        assert summary["playlist_item_attempts"] == [{"id": first_item, "attempts": 1, "passed": True},
                                                     {"id": second_item, "attempts": 1, "passed": True}]
        committed_stats = {entry["playlistItemID"]: entry for entry in metadata.call("BeginWatchingMultiplayerRoom", [room])}
        first_bins, second_bins = [0] * 13, [0] * 13
        first_bins[9], first_bins[10], second_bins[0], second_bins[10] = 1, 1, 1, 1
        for identity, bins, cumulative, last in [(first_item, first_bins, 1900000, peer_nm["id"]),
                                                 (second_item, second_bins, 1000000, peer_second["id"])]:
            assert committed_stats[identity] == {"playlistItemID": identity, "totalScoreDistribution": bins,
                                                "cumulativeScore": cumulative, "lastProcessedScoreID": last}
        peer.call("LeaveRoom")
        assert gateway.json("GET", first + f"/{host_nm['id']}", headers=peer_auth)["id"] == host_nm["id"]
        outside.call("JoinRoom", [room])
        assert gateway.json("GET", first, headers=outsider_auth)["scores"] == []
        assert gateway.json("GET", base + "/leaderboard", headers=outsider_auth)["leaderboard"] == []
        assert gateway.json("GET", base, headers=outsider_auth)["current_user_score"] is None
        gateway.request("GET", first + f"/{host_nm['id']}", headers=outsider_auth, expected=404)
        late_stats = outside_metadata.call("BeginWatchingMultiplayerRoom", [room])
        assert all(entry["totalScoreDistribution"] == [0] * 13 and entry["cumulativeScore"] == 0 and entry["lastProcessedScoreID"] == 0 for entry in late_stats)
        outside.call("LeaveRoom")
        host.call("LeaveRoom")
        assert gateway.json("PUT", second + f"/{rx_token}", rx_input, bearer)["id"] == room_rx["id"]

    return {"room_id": room, "normal_score_id": host_nm["id"], "peer_score_id": peer_nm["id"],
            "relax_score_id": room_rx["id"], "peer_second_score_id": peer_second["id"],
            "transport": "original_stdlib_json_sse", "http_requests": Client.request_count - initial_requests}


def run():
    assert os.environ.get("ILLYA_ALLOW_DISPOSABLE_TEST") == "1", "Set ILLYA_ALLOW_DISPOSABLE_TEST=1 for the disposable test environment"
    admin_name, admin_password = os.environ["TEST_ADMIN_USERNAME"], os.environ["TEST_ADMIN_PASSWORD"]
    player_name, player_password = os.environ["TEST_PLAYER_USERNAME"], os.environ["TEST_PLAYER_PASSWORD"]
    admin = Client(os.environ.get("TEST_SANCTUARY_URL", "http://127.0.0.1:18080"))
    gateway = Client(os.environ.get("TEST_RESONANCE_URL", "http://127.0.0.1:18081"))
    player_count = admin.json("GET", "/api/v1/status")["registered_players"]
    for path, check in [("/users", "0"), ("/users/", "1")]:
        fields = [("user[username]", "WebsiteOnlyFixture"),
                  ("user[user_email]", "website-only@example.test"),
                  ("user[password]", "never-create-this-account"), ("check", check)]
        if check == "0":
            body = urllib.parse.urlencode(fields).encode()
            headers = {"Content-Type": "application/x-www-form-urlencoded"}
        else:
            body, headers = multipart(fields)
        response, response_headers = gateway.request("POST", path, body, headers, expected=422)
        guide = json.loads(response)
        assert guide["url"] == gateway.base + "/register"
        assert guide["form_error"]["redirect"] == guide["url"]
        assert guide["url"] in guide["form_error"]["user"]["username"][0]
        assert b"never-create-this-account" not in response and b"website-only@example.test" not in response
        assert "Set-Cookie" not in response_headers
    assert admin.json("GET", "/api/v1/status")["registered_players"] == player_count
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
    assert gateway.json("GET", "/web/osu-getseasonal.php?fixture=ignored") == []
    assert gateway.request("GET", "/web/bancho_connect.php?fixture=ignored")[0] == b""
    avatar_headers = {"Host": "a.smoke.illya.test"}
    avatar, avatar_response = gateway.request("GET", f"/{player_id}", headers=avatar_headers)
    assert avatar.startswith(b"\x89PNG\r\n\x1a\n") and avatar_response["content-type"] == "image/png"
    gateway.request("GET", f"/{player_id}", headers={**avatar_headers, "If-None-Match": avatar_response["etag"]}, expected=304)
    gateway.request("GET", "/9223372036854775807", headers=avatar_headers, expected=404)
    response = gateway.request("POST", "/", packet(4), player_bancho)[0]
    assert response == b"", "Idle heartbeat must not emit unsolicited PONG packets"
    gateway.request("POST", "/", packet(4, b"invalid"), player_bancho, expected=400)
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
    score = {"ruleset_id": 0, "passed": True, "total_score": 1000000, "total_score_without_mods": 1000000, "accuracy": 1, "max_combo": 2, "rank": "X", "mods": [], "statistics": {"great": 2}, "maximum_statistics": {"great": 2}, "pauses": [], "user_id": 9999, "beatmap_id": 9999, "score_variant": "relax"}
    path = f"/api/v2/beatmaps/{map_id}/solo/scores/{token}"
    lazer = gateway.json("PUT", path, score, bearer)
    assert lazer["user_id"] == player_id and lazer["beatmap_id"] == map_id and lazer["verification"] == "pending" and lazer["pp"] > 0
    assert lazer["score_variant"] == "vanilla", "Untrusted variant overrode native mods"
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

    # RX uses the original ruleset, genuine legacy encryption and native solo
    # tokens. A worse score is submitted first; the later, better play must win
    # the same raw-score tie through PP rather than submission order.
    # Two circles are sufficient for protocol/scale checks above, but can yield
    # zero Relax PP. Use a longer original fixture for meaningful PP ordering.
    primary_map_id = map_id
    rx_count = 80
    rx_objects = [(80 + index % 4 * 100, 80 if index // 4 % 2 == 0 else 280, 1000 + index * 240) for index in range(rx_count)]
    rx_fixture = ("osu file format v14\n[General]\nMode:0\n[Metadata]\nTitle:Independent RX HTTP fixture " + uuid.uuid4().hex + "\nArtist:Illyaverse\nCreator:Test\nVersion:Original zigzag\n[Difficulty]\nHPDrainRate:5\nCircleSize:4\nOverallDifficulty:7\nApproachRate:8\nSliderMultiplier:1.4\nSliderTickRate:1\n[TimingPoints]\n0,480,4,2,1,100,1,0\n[HitObjects]\n" + "".join(f"{x},{y},{time},1,0,0:0:0:0:\n" for x, y, time in rx_objects)).encode()
    rx_archive = io.BytesIO()
    with zipfile.ZipFile(rx_archive, "w", zipfile.ZIP_DEFLATED) as package:
        package.writestr("independent-rx.osu", rx_fixture)
    upload_body, upload_headers = multipart([], [("file", "independent-rx.osz", rx_archive.getvalue())])
    rx_map = admin.json("POST", "/api/v1/beatmaps/upload", upload_body, upload_headers)["items"][0]
    map_id, map_hash = rx_map["id"], rx_map["md5"]
    admin.json("PATCH", f"/api/v1/admin/beatmaps/{map_id}", {"status": "ranked"})
    assert gateway.request("GET", f"/osu/{map_id}")[0] == rx_fixture
    frames = lzma.compress(("".join(f"{1000 if index == 0 else 240}|{x}|{y}|1," for index, (x, y, _) in enumerate(rx_objects)) + "-12345|0|0|12345,").encode(), format=lzma.FORMAT_ALONE)
    fields = [(name, map_hash if name == "bmk" else value) for name, value in fields]
    nm_token_fields = urllib.parse.urlencode({"beatmap_hash": map_hash, "version_hash": "independent-rx-nm-baseline", "ruleset_id": 0}).encode()
    nm_token = gateway.json("POST", f"/api/v2/beatmaps/{map_id}/solo/scores", nm_token_fields, dict(bearer, **{"Content-Type": "application/x-www-form-urlencoded"}))["id"]
    nm_baseline = gateway.json("PUT", f"/api/v2/beatmaps/{map_id}/solo/scores/{nm_token}", dict(score, max_combo=rx_count, statistics={"great": rx_count}, maximum_statistics={"great": rx_count}), bearer)
    assert nm_baseline["score_variant"] == "vanilla"
    admin.json("PATCH", f"/api/v1/admin/scores/{nm_baseline['id']}", {"verification": "verified"})
    vanilla_stats = gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"]
    vanilla_site_stats = admin.json("GET", f"/api/v1/users/{player_id}?mode=0")["statistics"]
    admin_oauth = gateway.json("POST", "/oauth/token", {"grant_type": "password", "username": admin_name, "password": admin_password})
    admin_bearer = {"Authorization": "Bearer " + admin_oauth["access_token"]}

    def encrypted_rx(n300=None, n100=0, mode=0, expected=200):
        n300 = rx_count if n300 is None else n300
        rank = "X" if n100 == 0 else "B"
        rx_checksum = digest(f"chickenmcnuggets{n300 + n100}o1500smustard00uu{map_hash}{rx_count}True{player_name}0{rank}128QTrue{mode}{version}{played_at}{client_hash}")
        rx_envelope = f"{map_hash}:{player_name}:{rx_checksum}:{n300}:{n100}:0:0:0:0:0:{rx_count}:True:{rank}:128:True:{mode}:{played_at}:b{version}"
        rx_fields = [(name, encrypt(rx_envelope) if name == "score" else value) for name, value in fields]
        rx_body, rx_headers = multipart(rx_fields, [("score", "replay.osr", frames)])
        response = gateway.request("POST", "/web/osu-submit-modular-selector.php", rx_body, rx_headers, expected=expected)[0]
        if expected != 200:
            return None
        found = re.search(rb"onlineScoreId:(\d+)", response)
        assert found, "Encrypted RX submission omitted its online score ID"
        score_id = int(found.group(1))
        retry = gateway.request("POST", "/web/osu-submit-modular-selector.php", rx_body, rx_headers)[0]
        assert f"onlineScoreId:{score_id}|".encode() in retry, "Encrypted RX retry created another score"
        return score_id

    def native_rx(auth, mode=0, weak=False, expected=200):
        token_fields = urllib.parse.urlencode({"beatmap_hash": map_hash, "version_hash": "independent-rx-smoke", "ruleset_id": mode}).encode()
        rx_token = gateway.json("POST", f"/api/v2/beatmaps/{map_id}/solo/scores", token_fields, dict(auth, **{"Content-Type": "application/x-www-form-urlencoded"}))["id"]
        payload = {"ruleset_id": mode, "passed": True, "total_score": 0,
                   "total_score_without_mods": 833333 if weak else 1000000,
                   "accuracy": 5 / 6 if weak else 1, "max_combo": rx_count,
                   "rank": "B" if weak else "X", "mods": [{"acronym": "RX"}],
                   "statistics": {"great": 60, "ok": 20} if weak else {"great": rx_count},
                   "maximum_statistics": {"great": rx_count}, "pauses": [],
                   "score_variant": "vanilla", "user_id": 9999, "beatmap_id": 9999}
        rx_path = f"/api/v2/beatmaps/{map_id}/solo/scores/{rx_token}"
        if expected != 200:
            gateway.request("PUT", rx_path, payload, auth, expected=expected)
            return None
        submitted = gateway.json("PUT", rx_path, payload, auth)
        assert submitted["score_variant"] == "relax" and submitted["ruleset_id"] == mode
        assert submitted["beatmap_id"] == map_id and submitted["verification"] == "pending"
        assert gateway.json("PUT", rx_path, payload, auth)["id"] == submitted["id"], "Native RX retry created another score"
        gateway.request("PUT", rx_path, dict(payload, mods=[]), auth, expected=409)
        return submitted

    rx_weak_id = encrypted_rx(n300=60, n100=20)
    rx_stable_id = encrypted_rx()
    rx_lazer = native_rx(bearer)
    rx_admin = native_rx(admin_bearer, weak=True)
    rx_player_ids = {rx_weak_id, rx_stable_id, rx_lazer["id"]}
    rx_all_ids = rx_player_ids | {rx_admin["id"]}
    rx_records = {score_id: admin.json("GET", f"/api/v1/scores/{score_id}") for score_id in rx_all_ids}
    for score_id, stored in rx_records.items():
        assert stored["mode"] == 0 and stored["mods"] == 128 and stored["score_variant"] == "relax"
        assert stored["pp_eligible"] and stored["pp"] > 0 and stored["pp_calculation_version"] == "rosu_pp4.v1", (score_id, stored["pp_eligible"], stored["pp"], stored["pp_calculation_version"])
        assert stored["score"] == 0 and stored["standardised_score"] is None and stored["classic_score"] is None
        assert stored["standardised_without_mods"] is None and stored["score_scale_status"] == "unavailable"
        assert stored["score_scale_version"] == "relax_score_scale.unavailable.v1"
        assert stored["user_id"] == (admin_id if score_id == rx_admin["id"] else player_id)
    assert rx_records[rx_stable_id]["pp"] > rx_records[rx_weak_id]["pp"], "Fixture did not distinguish RX PP"
    assert rx_records[rx_lazer["id"]]["native_submission"]["mods"] == [{"acronym": "RX"}]
    assert rx_records[rx_lazer["id"]]["api_mods"] == [{"acronym": "RX"}]
    assert "score_variant" not in rx_records[rx_lazer["id"]]["native_submission"], "Untrusted native variant was persisted"
    stored_list = admin.json("GET", f"/api/v1/scores?user_id={player_id}&mode=0&variant=relax")
    assert {item["id"] for item in stored_list["items"]} == rx_player_ids and stored_list["total"] == 3

    rx_query = urllib.parse.urlencode({"u": player_name, "h": digest(player_password), "c": map_hash, "m": 0, "mods": 128})
    rx_line = re.compile(rf"^{rx_stable_id}\|{re.escape(player_name)}\|", re.MULTILINE)
    assert not rx_line.search(gateway.request("GET", "/web/osu-osz2-getscores.php?" + rx_query)[0].decode())
    assert gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&variant=relax", headers=bearer)["scores"] == []
    assert admin.json("GET", f"/api/v1/beatmaps/{map_id}/scores?mode=0&variant=relax")["items"] == []
    pending_rx = admin.json("GET", f"/api/v1/users/{player_id}?mode=0&variant=relax")["statistics"]
    assert pending_rx["pp"] == 0 and pending_rx["play_count"] == 0

    rx_replay = gateway.request("GET", f"/api/v2/scores/{rx_stable_id}/download", headers=bearer)[0]
    replay = replay_metadata(rx_replay)
    assert replay["mode"] == 0 and replay["mods"] == 128 and replay["score_id"] == rx_stable_id
    assert replay["map_hash"] == map_hash and replay["username"] == player_name and replay["hits"] == (rx_count, 0, 0, 0, 0, 0)
    assert replay["raw"] == 0 and replay["frames"] == frames
    rx_replay_query = urllib.parse.urlencode({"u": player_name, "h": digest(player_password), "c": rx_stable_id, "m": 0})
    assert gateway.request("GET", "/web/osu-getreplay.php?" + rx_replay_query)[0] == frames
    gateway.request("GET", "/web/osu-getreplay.php?" + rx_replay_query.replace("m=0", "m=1"), expected=404)

    for score_id in rx_all_ids:
        admin.json("PATCH", f"/api/v1/admin/scores/{score_id}", {"verification": "verified"})
    rx_board = gateway.request("GET", "/web/osu-osz2-getscores.php?" + rx_query)[0].decode()
    assert rx_line.search(rx_board) and not re.search(rf"^{rx_weak_id}\|", rx_board, re.MULTILINE)
    assert not score_line.search(rx_board) and not v2_line.search(rx_board)
    nm_board = gateway.request("GET", "/web/osu-osz2-getscores.php?" + leaderboard_query)[0].decode()
    assert score_line.search(nm_board) and not rx_line.search(nm_board)
    rx_best = max((rx_records[score_id] for score_id in rx_player_ids), key=lambda item: (item["pp"], -item["id"]))
    assert rx_best["pp"] > rx_records[rx_admin["id"]]["pp"]
    site_board = admin.json("GET", f"/api/v1/beatmaps/{map_id}/scores?mode=0&variant=relax")
    assert site_board["score_variant"] == "relax" and site_board["mode"] == 0
    assert [item["id"] for item in site_board["items"]] == [rx_best["id"], rx_admin["id"]]
    native_board = gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&variant=relax", headers=bearer)
    assert native_board["score_count"] == 2 and native_board["score_variant"] == "relax" and native_board["ruleset_id"] == 0
    assert [item["id"] for item in native_board["scores"]] == [rx_best["id"], rx_admin["id"]]
    assert native_board["user_score"]["position"] == 1
    assert [item["id"] for item in gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu", headers=bearer)["scores"]] == [nm_baseline["id"]]
    assert [item["id"] for item in admin.json("GET", f"/api/v1/beatmaps/{map_id}/scores?mode=0")["items"]] == [nm_baseline["id"]]
    for native_score in native_board["scores"]:
        assert native_score["score_variant"] == "relax" and native_score["ruleset_id"] == 0
        assert native_score["score_scale_status"] == "unavailable" and native_score["classic_total_score"] is None
        assert any(mod["acronym"] == "RX" for mod in native_score["mods"])
    assert gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&mods[]=RX", headers=bearer)["scores"][0]["id"] == rx_lazer["id"]
    assert gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&mods[]=RX&mods[]=CL", headers=bearer)["scores"][0]["id"] == rx_stable_id
    gateway.request("GET", f"/api/v2/beatmaps/{map_id}/scores?mode=osu&mods[]=RX&variant=vanilla", headers=bearer, expected=400)

    rx_stats = admin.json("GET", f"/api/v1/users/{player_id}?mode=0&variant=relax")["statistics"]
    rx_native_stats = gateway.json("GET", "/api/v2/me/osu?variant=relax", headers=bearer)["statistics"]
    assert rx_stats["play_count"] == 3 and rx_native_stats["play_count"] == 3
    assert abs(rx_stats["pp"] - rx_best["pp"]) < 1e-9 and abs(rx_native_stats["pp"] - rx_best["pp"]) < 1e-9
    assert rx_stats["total_score"] is None and rx_stats["ranked_score"] is None and not rx_stats["score_totals_complete"]
    assert rx_stats["score_scale"] == "unavailable_for_relax" and rx_stats["global_rank"] == rx_native_stats["global_rank"] == 1
    assert admin.json("GET", f"/api/v1/users/{player_id}?mode=0")["statistics"] == vanilla_site_stats
    assert gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"] == vanilla_stats
    site_ranking = admin.json("GET", "/api/v1/leaderboard?mode=0&variant=relax")
    native_ranking = gateway.json("GET", "/api/v2/rankings/osu/performance?variant=relax", headers=bearer)
    assert site_ranking["items"][0]["id"] == player_id and site_ranking["items"][0]["rank"] == 1
    assert native_ranking["ranking"][0]["user"]["id"] == player_id and native_ranking["ranking"][0]["global_rank"] == 1
    assert abs(site_ranking["items"][0]["pp"] - rx_best["pp"]) < 1e-9
    assert abs(native_ranking["ranking"][0]["pp"] - rx_best["pp"]) < 1e-9
    rx_best_scores = gateway.json("GET", f"/api/v2/users/{player_id}/scores/best?mode=osu&variant=relax", headers=bearer)
    assert [item["id"] for item in rx_best_scores] == [rx_best["id"]]
    assert all(item["score_variant"] == "vanilla" for item in gateway.json("GET", f"/api/v2/users/{player_id}/scores/best?mode=osu", headers=bearer))

    # Supported converted RX rulesets retain their own PP/play-count partition.
    rx_converted_ids = []
    for mode, native_mode in [(1, "taiko"), (2, "fruits")]:
        stable_converted = encrypted_rx(mode=mode)
        native_converted = native_rx(bearer, mode=mode)
        converted_ids = {stable_converted, native_converted["id"]}
        rx_converted_ids.extend(sorted(converted_ids))
        converted_records = [admin.json("GET", f"/api/v1/scores/{score_id}") for score_id in converted_ids]
        for stored in converted_records:
            assert stored["mode"] == mode and stored["score_variant"] == "relax" and stored["pp_eligible"] and stored["pp"] > 0
            admin.json("PATCH", f"/api/v1/admin/scores/{stored['id']}", {"verification": "verified"})
        converted_stats = admin.json("GET", f"/api/v1/users/{player_id}?mode={mode}&variant=relax")["statistics"]
        assert converted_stats["play_count"] == 2 and abs(converted_stats["pp"] - max(item["pp"] for item in converted_records)) < 1e-9
        assert gateway.json("GET", f"/api/v2/me/{native_mode}?variant=relax", headers=bearer)["statistics"]["play_count"] == 2
        assert admin.json("GET", f"/api/v1/users/{player_id}?mode={mode}")["statistics"]["pp"] == 0
        assert gateway.json("GET", f"/api/v2/beatmaps/{map_id}/scores?mode={native_mode}&variant=relax", headers=bearer)["score_count"] == 1
    assert admin.json("GET", f"/api/v1/users/{player_id}?mode=0&variant=relax")["statistics"] == rx_stats
    assert gateway.json("GET", "/api/v2/me/osu", headers=bearer)["statistics"] == vanilla_stats
    encrypted_rx(mode=3, expected=400)
    native_rx(bearer, mode=3, expected=400)
    admin.request("GET", f"/api/v1/users/{player_id}?mode=3&variant=relax", expected=400)
    gateway.request("GET", "/api/v2/me/mania?variant=relax", headers=bearer, expected=400)
    assert admin.json("GET", f"/api/v1/scores?user_id={player_id}&mode=3")["items"] == []

    room_scores = native_room_smoke(admin, gateway, bearer, player_id, map_id, map_hash, rx_objects)
    rotated = gateway.json("POST", "/oauth/token", {"grant_type": "refresh_token", "refresh_token": oauth["refresh_token"]})
    gateway.request("GET", "/api/v2/me", headers=bearer, expected=401)
    assert gateway.json("GET", "/api/v2/me", headers={"Authorization": "Bearer " + rotated["access_token"]})["id"] == player_id
    gateway.request("POST", "/", packet(2), player_bancho)
    after_logout = gateway.request("POST", "/", packet(4), player_bancho)[0]
    assert any(i == 86 for i, _ in packets(after_logout)), "Logged out stable session remained valid"
    checks = ["shared auth", "stable binary login/ping/chat/logout", "lazer OAuth/refresh", "lazer checksum/set lookup/archive/score collections", "Classic profile totals and standardized mixed-client ranking", "stable ScoreV1/ScoreV2 board partition", "RBAC", "friend/block/DM settings", "stable password room/host authority/slot identity", "stable ready/start/load/score/complete/transfer/leave", "stable spectator frame relay", "lazer score token/idempotency", "stable Rijndael256 submission/checksum", "server PP", "replay round trip", "pending rankings", "moderation", "encrypted stable RX and native solo RX", "RX metadata and unavailable common scale", "RX retry deduplication and native mod binding", "RX pending isolation and fixture moderation", "RX replay original ruleset/mod binding", "RX PP-sorted boards and best-per-map totals", "NM/RX API and gateway rank/stat isolation", "osu/taiko/catch RX partition isolation", "mania RX rejection and untrusted variant derivation"]
    checks.extend(["lazer HeadToHead frozen loading token lifecycle", "real room ingress and Sanctuary PP",
                   "room exact retry and changed-payload conflict", "room/solo and room/item/user scope isolation",
                   "private frozen peers and late-joiner empty history", "room index/cursors/user/leaderboard/native summaries",
                   "RX optional participant room mods and native total", "metadata stats/events and exact ended spectator processing"])
    print(json.dumps({"status": "passed", "beatmap_id": primary_map_id, "rx_beatmap_id": map_id, "stable_score_id": stable_id, "lazer_score_id": lazer["id"], "rx_stable_score_id": rx_stable_id, "rx_lazer_score_id": rx_lazer["id"], "rx_converted_score_ids": rx_converted_ids, "room_scores": room_scores, "http_requests": Client.request_count, "check_count": len(checks), "checks": checks}))


if __name__ == "__main__":
    run()
