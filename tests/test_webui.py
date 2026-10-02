"""Tests for wallmotion.webui - pure helpers + live localhost server."""

import json
import os
import urllib.error
import urllib.request

from wallmotion.webui import (
    WebLibraryServer,
    format_size,
    list_media,
    safe_name,
    thumbnail_command,
)


class TestFormatSize:
    def test_mb(self):
        assert format_size(2097152) == "2.0 MB"

    def test_kb(self):
        assert format_size(2048) == "2 KB"

    def test_garbage(self):
        assert format_size("x") == ""


class TestListMedia:
    def test_lists_images_and_videos(self, tmp_path):
        (tmp_path / "a.mp4").write_bytes(b"x" * 10)
        (tmp_path / "b.jpg").write_bytes(b"y" * 20)
        (tmp_path / "c.txt").write_bytes(b"z")
        items = list_media(str(tmp_path))
        by_name = {i["name"]: i for i in items}
        assert by_name["a.mp4"]["kind"] == "video"
        assert by_name["b.jpg"]["kind"] == "image"
        assert "c.txt" not in by_name
        assert by_name["a.mp4"]["size"] == 10

    def test_missing_dir(self, tmp_path):
        assert list_media(str(tmp_path / "nope")) == []


class TestSafeName:
    def test_ok(self, tmp_path):
        (tmp_path / "a.mp4").write_bytes(b"x")
        assert safe_name(str(tmp_path), "a.mp4") is not None

    def test_traversal_rejected(self, tmp_path):
        assert safe_name(str(tmp_path), "../a.mp4") is None
        assert safe_name(str(tmp_path), "sub/a.mp4") is None
        assert safe_name(str(tmp_path), "") is None

    def test_missing_rejected(self, tmp_path):
        assert safe_name(str(tmp_path), "nope.mp4") is None


class TestThumbnailCommand:
    def test_shape(self, tmp_path):
        cmd = thumbnail_command(str(tmp_path / "a.mp4"),
                                str(tmp_path / "a.jpg"))
        if cmd is None:
            return  # no ffmpeg here - builder correctly declines
        assert os.path.splitext(os.path.basename(cmd[0]).lower())[0].startswith(
            "ffmpeg")
        assert "-frames:v" in cmd and "1" in cmd
        assert str(tmp_path / "a.jpg") in cmd


class TestLiveServer:
    def _server(self, tmp_path):
        (tmp_path / "v.mp4").write_bytes(b"x" * 100)
        applied = []
        stopped = []
        server = WebLibraryServer(
            apply_fn=lambda p: applied.append(p) or True,
            stop_fn=lambda: stopped.append(True),
            port=0, directory=str(tmp_path))
        port = server.start()
        assert port
        return server, applied, stopped

    def _get(self, port, path):
        try:
            with urllib.request.urlopen(
                    f"http://127.0.0.1:{port}{path}", timeout=10) as r:
                return r.status, r.read()
        except urllib.error.HTTPError as e:
            return e.code, e.read()

    def _post(self, port, path, obj):
        data = json.dumps(obj).encode()
        req = urllib.request.Request(
            f"http://127.0.0.1:{port}{path}", data=data,
            headers={"Content-Type": "application/json"})
        try:
            with urllib.request.urlopen(req, timeout=10) as r:
                return r.status, json.loads(r.read())
        except urllib.error.HTTPError as e:
            return e.code, json.loads(e.read() or b"{}")

    def test_index_and_files(self, tmp_path):
        server, _applied, _stopped = self._server(tmp_path)
        try:
            status, body = self._get(server.port, "/")
            assert status == 200 and b"WallMotion library" in body
            status, body = self._get(server.port, "/api/files")
            assert status == 200
            assert json.loads(body) == [
                {"name": "v.mp4", "kind": "video", "size": 100}]
        finally:
            server.stop()

    def test_set_and_stop(self, tmp_path):
        server, applied, stopped = self._server(tmp_path)
        try:
            status, obj = self._post(server.port, "/api/set",
                                      {"name": "v.mp4"})
            assert status == 200 and obj == {"ok": True}
            assert applied and applied[0].endswith("v.mp4")
            status, obj = self._post(server.port, "/api/stop", {})
            assert status == 200 and obj == {"ok": True}
            assert stopped == [True]
        finally:
            server.stop()

    def test_traversal_blocked(self, tmp_path):
        server, _applied, _stopped = self._server(tmp_path)
        try:
            status, _body = self._post(server.port, "/api/set",
                                       {"name": "../secret"})
            assert status == 404
        finally:
            server.stop()

    def test_bound_to_localhost(self, tmp_path):
        server, _a, _s = self._server(tmp_path)
        try:
            assert server.url.startswith("http://127.0.0.1:")
        finally:
            server.stop()
