#!/usr/bin/env python3
"""Download a synthetic HDR fixture, stop its server, then verify local HDR presentation.

This never signs in to Plex or writes viewing history. Run in the Deck's Gaming Mode.
It is a backend probe, not evidence that the native player integration is finished.
"""
import hashlib
import http.server
import json
import os
from pathlib import Path
import subprocess
import threading

root = Path.home() / ".local/share/plxnative-steamdeck"
fixture = root / "fixtures/hdr10-test.mkv"
assert fixture.is_file(), "Generate the synthetic HDR fixture first"


class Handler(http.server.BaseHTTPRequestHandler):
    def do_GET(self):
        if self.path != "/library/parts/fixture/1/hdr.mkv":
            self.send_error(404)
            return
        data = fixture.read_bytes()
        self.send_response(200)
        self.send_header("Content-Type", "video/x-matroska")
        self.send_header("Content-Length", str(len(data)))
        self.send_header("ETag", '"' + hashlib.sha256(data).hexdigest() + '"')
        self.end_headers()
        self.wfile.write(data)

    def log_message(self, *_args):
        pass


server = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Handler)
thread = threading.Thread(target=server.serve_forever, daemon=True)
thread.start()
request = {
    "identity": {"profile": "synthetic-device-test", "server": "fixture-server",
                 "rating_key": "fixture", "part": "/library/parts/fixture/1/hdr.mkv"},
    "title": "Synthetic HDR10 test",
    "expected_bytes": fixture.stat().st_size,
    "url": f"http://127.0.0.1:{server.server_port}/library/parts/fixture/1/hdr.mkv",
}
try:
    result = subprocess.run([str(root / "bin/deck-download"), str(root / "test-downloads")],
                            input=json.dumps(request), text=True, capture_output=True, timeout=60)
    assert result.returncode == 0, result.stderr
    lines = result.stdout.splitlines()
    entry = json.loads(lines[0])
    downloaded = Path(lines[1])
    assert entry["state"] == "complete"
    assert hashlib.sha256(downloaded.read_bytes()).digest() == hashlib.sha256(fixture.read_bytes()).digest()
finally:
    server.shutdown()
    server.server_close()
    thread.join()
print("PASS: original HDR file downloaded byte-for-byte; fixture server is stopped", flush=True)

command = [
    "flatpak", "run", "--filesystem=host-os:ro", "--filesystem=xdg-run/gamescope-0",
    "--env=GAMESCOPE_WAYLAND_DISPLAY=gamescope-0", "--env=ENABLE_GAMESCOPE_WSI=1",
    "--env=STEAM_GAMESCOPE_HDR_SUPPORTED=1",
    f"--env=VK_LAYER_PATH={root}/vulkan",
    "--env=VK_INSTANCE_LAYERS=VK_LAYER_FROG_gamescope_wsi_x86_64", "io.mpv.Mpv",
    "--no-config", "--vo=gpu-next", "--gpu-api=vulkan", "--gpu-context=x11vk",
    "--target-colorspace-hint=yes", "--hwdec=vaapi", "--fullscreen", "--no-audio",
    "--msg-level=all=debug", str(downloaded),
]
env = dict(os.environ, DISPLAY=":1", XDG_RUNTIME_DIR=f"/run/user/{os.getuid()}")
log_path = root / "download-hdr-test.log"
with log_path.open("w") as log:
    result = subprocess.run(command, env=env, stdout=log, stderr=subprocess.STDOUT, timeout=25)
log = log_path.read_text()
assert result.returncode == 0, f"Player failed; see {log_path}"
assert "Using hardware decoding (vaapi)." in log
assert "colorspace: VK_COLOR_SPACE_HDR10_ST2084_EXT" in log
assert "VkHdrMetadataEXT" in log
assert "Exiting... (End of file)" in log
print("PASS: downloaded file played to EOF with server stopped, VAAPI and HDR10/PQ swapchain", flush=True)
