"""Local web library: browse and apply downloaded wallpapers from a page.

Stdlib only (http.server): a localhost panel with a grid of downloaded
videos (ffmpeg thumbnails) and images, one-click apply, stop button.
Useful for remote control on the same machine and headless use.
Binds 127.0.0.1 only; file access is jailed to the media directory.
"""

from __future__ import annotations

import functools
import http.server
import json
import mimetypes
import os
import socketserver
import threading
import urllib.parse

from wallmotion.paths import app_dirs
from wallmotion.utils import IMAGE_EXTS, VIDEO_EXTS

DEFAULT_PORT = 8765
THUMB_SECONDS = 1


def format_size(size: int) -> str:
    """'2.0 MB' style label. Pure, unit-tested."""
    try:
        size = int(size)
    except Exception:
        return ""
    if size >= 1048576:
        return f"{size / 1048576:.1f} MB"
    return f"{max(1, size // 1024)} KB"


def media_dir() -> str:
    try:
        return str(app_dirs()["downloads"])
    except Exception:
        return os.path.abspath("downloads")


def thumbs_dir() -> str:
    try:
        return str(app_dirs()["log"].parent / "thumbs")
    except Exception:
        import tempfile
        return os.path.join(tempfile.gettempdir(), "wallmotion-thumbs")


def list_media(directory: str | None = None) -> list:
    """[{name, kind, size}] of images/videos in the media dir. Pure I/O."""
    directory = directory or media_dir()
    items = []
    try:
        names = sorted(os.listdir(directory))
    except Exception:
        return items
    for name in names:
        try:
            ext = os.path.splitext(name)[1].lower()
            if ext in VIDEO_EXTS:
                kind = "video"
            elif ext in IMAGE_EXTS:
                kind = "image"
            else:
                continue
            full = os.path.join(directory, name)
            if not os.path.isfile(full):
                continue
            items.append({"name": name, "kind": kind,
                          "size": os.path.getsize(full)})
        except Exception:
            continue
    return items


def safe_name(directory: str, name: str) -> str | None:
    """Absolute path for a requested name, or None (traversal/missing)."""
    try:
        if not name or os.path.basename(name) != name:
            return None
        full = os.path.abspath(os.path.join(directory, name))
        if os.path.dirname(full) != os.path.abspath(directory):
            return None
        if not os.path.isfile(full):
            return None
        return full
    except Exception:
        return None


def thumbnail_command(video_path: str, out_path: str) -> list | None:
    """ffmpeg command for a 320px JPEG thumbnail. None without ffmpeg."""
    try:
        from wallmotion.youtube import _ffmpeg_exe
        exe = _ffmpeg_exe()
        if not exe:
            return None
        return [exe, "-y", "-v", "error", "-ss", str(THUMB_SECONDS),
                "-i", video_path, "-frames:v", "1",
                "-vf", "scale=320:-1", out_path]
    except Exception:
        return None


def ensure_thumbnail(video_path: str, cache_dir: str | None = None) -> str | None:
    """Generate (once) and return the thumbnail path, or None."""
    try:
        cache_dir = cache_dir or thumbs_dir()
        os.makedirs(cache_dir, exist_ok=True)
        base = os.path.splitext(os.path.basename(video_path))[0]
        out = os.path.join(cache_dir, base + ".jpg")
        if os.path.exists(out) and (
                os.path.getmtime(out) >= os.path.getmtime(video_path)):
            return out
        cmd = thumbnail_command(video_path, out)
        if not cmd:
            return None
        import subprocess
        proc = subprocess.run(cmd, capture_output=True, timeout=60)
        if proc.returncode == 0 and os.path.exists(out):
            return out
        return None
    except Exception:
        return None


PAGE = """<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>WallMotion library</title>
<style>
body{background:#1b1c22;color:#eceef2;font-family:sans-serif;margin:0;padding:24px}
h1{font-size:20px}.bar{display:flex;gap:8px;margin-bottom:16px}
button{background:#7c5cff;color:#fff;border:none;border-radius:8px;padding:10px 16px;font-size:14px;cursor:pointer}
button.ghost{background:#25262e}
.grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:16px}
.card{background:#25262e;border-radius:12px;overflow:hidden;cursor:pointer}
.card img{width:100%;height:130px;object-fit:cover;display:block;background:#000}
.card .n{padding:8px 12px;font-size:13px;white-space:nowrap;overflow:hidden;text-overflow:ellipsis}
.card .s{padding:0 12px 10px;color:#9195a3;font-size:12px}
</style>
</head>
<body>
<h1>WallMotion library</h1>
<div class="bar"><button class="ghost" onclick="stop()">Stop wallpaper</button><span id="st"></span></div>
<div class="grid" id="g"></div>
<script>
function fmt(n){if(n>1048576)return (n/1048576).toFixed(1)+" MB";return Math.round(n/1024)+" KB"}
async function load(){
  const r=await fetch("api/files");const fs=await r.json();
  const g=document.getElementById("g");g.innerHTML="";
  for(const f of fs){
    const d=document.createElement("div");d.className="card";
    d.onclick=()=>set(f.name);
    const img=f.kind==="video"
      ?`api/thumb?name=${encodeURIComponent(f.name)}`
      :`api/file?name=${encodeURIComponent(f.name)}`;
    d.innerHTML=`<img loading="lazy" src="${img}">`
      +`<div class="n">${f.name}</div>`
      +`<div class="s">${f.kind} · ${fmt(f.size)}</div>`;
    g.appendChild(d);
  }
  document.getElementById("st").textContent=fs.length+" files";
}
async function set(name){
  await fetch("api/set",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({name})});
}
async function stop(){await fetch("api/stop",{method:"POST"});}
load();
</script>
</body>
</html>
"""


class _Handler(http.server.BaseHTTPRequestHandler):
    server_version = "WallMotion/1"

    def _json(self, obj, code: int = 200):
        try:
            body = json.dumps(obj).encode()
            self.send_response(code)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)
        except Exception:
            pass

    def _send_file(self, path: str, content_type: str | None = None):
        try:
            ctype = content_type or mimetypes.guess_type(path)[0] or \
                "application/octet-stream"
            size = os.path.getsize(path)
            self.send_response(200)
            self.send_header("Content-Type", ctype)
            self.send_header("Content-Length", str(size))
            self.end_headers()
            with open(path, "rb") as f:
                while True:
                    chunk = f.read(65536)
                    if not chunk:
                        break
                    self.wfile.write(chunk)
        except Exception:
            pass

    def do_GET(self):  # noqa: N802 (BaseHTTPRequestHandler naming)
        try:
            parsed = urllib.parse.urlparse(self.path)
            query = urllib.parse.parse_qs(parsed.query)
            if parsed.path in ("/", "/index.html"):
                body = PAGE.encode()
                self.send_response(200)
                self.send_header("Content-Type", "text/html; charset=utf-8")
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)
                return
            if parsed.path == "/api/files":
                self._json(list_media(self.server.media_dir))
                return
            if parsed.path in ("/api/file", "/api/thumb"):
                name = (query.get("name") or [""])[0]
                full = safe_name(self.server.media_dir, name)
                if not full:
                    self._json({"error": "not found"}, 404)
                    return
                if parsed.path == "/api/thumb":
                    ext = os.path.splitext(full)[1].lower()
                    if ext not in VIDEO_EXTS:
                        self._json({"error": "images need no thumbnail"}, 400)
                        return
                    thumb = ensure_thumbnail(full, self.server.thumbs_dir)
                    if not thumb:
                        self._json({"error": "no thumbnail"}, 500)
                        return
                    self._send_file(thumb, "image/jpeg")
                    return
                self._send_file(full)
                return
            self._json({"error": "unknown"}, 404)
        except Exception:
            pass

    def do_POST(self):  # noqa: N802 (BaseHTTPRequestHandler naming)
        try:
            parsed = urllib.parse.urlparse(self.path)
            length = int(self.headers.get("Content-Length") or 0)
            body = self.rfile.read(length) if length > 0 else b""
            if parsed.path == "/api/set":
                try:
                    name = json.loads(body or b"{}").get("name", "")
                except Exception:
                    name = ""
                full = safe_name(self.server.media_dir, name)
                if not full:
                    self._json({"error": "not found"}, 404)
                    return
                try:
                    ok = self.server.apply_fn(full)
                except Exception:
                    ok = False
                self._json({"ok": bool(ok)})
                return
            if parsed.path == "/api/stop":
                try:
                    self.server.stop_fn()
                except Exception:
                    pass
                self._json({"ok": True})
                return
            self._json({"error": "unknown"}, 404)
        except Exception:
            pass

    def log_message(self, *args):
        pass


class WebLibraryServer:
    """Threaded localhost server. apply_fn(path)->bool, stop_fn()."""

    def __init__(self, apply_fn=None, stop_fn=None, port: int = DEFAULT_PORT,
                 directory: str | None = None):
        self.apply_fn = apply_fn or (lambda _p: False)
        self.stop_fn = stop_fn or (lambda: None)
        self.media_dir = directory or media_dir()
        self.thumbs_dir = thumbs_dir()
        self.port = port
        self._server = None
        self._thread = None

    def start(self) -> int | None:
        """Start serving in a daemon thread. Returns the port, or None."""
        try:
            handler = functools.partial(_Handler)
            self._server = socketserver.ThreadingTCPServer(
                ("127.0.0.1", self.port), handler)
            self._server.media_dir = self.media_dir
            self._server.thumbs_dir = self.thumbs_dir
            self._server.apply_fn = self.apply_fn
            self._server.stop_fn = self.stop_fn
            self._server.daemon_threads = True
            self._server.allow_reuse_address = True
            self.port = self._server.server_address[1]
            self._thread = threading.Thread(target=self._server.serve_forever,
                                            kwargs={"poll_interval": 0.5},
                                            daemon=True)
            self._thread.start()
            return self.port
        except Exception:
            self._server = None
            return None

    def stop(self) -> None:
        server, self._server = self._server, None
        if server is None:
            return
        try:
            server.shutdown()
        except Exception:
            pass
        try:
            server.server_close()
        except Exception:
            pass

    @property
    def url(self) -> str | None:
        if self._server is None:
            return None
        return f"http://127.0.0.1:{self.port}/"
