"""A playback-scoped Gamescope refresh lease; no media URLs or account data.

Only the internal panel and rates advertised by Gamescope are eligible. The
helper survives a native UI crash until mpv's private IPC closes, then restores
the previous refresh/limiter setting. User changes during playback take priority.
Protocol: ValveSoftware/gamescope/protocol/gamescope-control.xml (version 2).
"""
import json
import math
import os
import select
import signal
import socket
import struct
import subprocess
import sys
import time


def words(*values):
    return struct.pack('=' + 'I' * len(values), *values)


def string(value):
    data = value.encode() + b'\0'
    return words(len(data)) + data + bytes((-len(data)) % 4)


def field(data, offset):
    size, = struct.unpack_from('=I', data, offset)
    start = offset + 4
    if start + size > len(data):
        raise ValueError('Truncated Wayland field')
    return data[start:start + size], start + ((size + 3) & ~3)


def cadence(fps, rates):
    if not isinstance(fps, (int, float)) or not math.isfinite(fps):
        return None
    nominal = round(fps)
    if nominal not in (24, 25, 30, 50, 60) or abs(fps - nominal) > .07:
        return None
    matches = [rate for rate in rates if 40 <= rate <= 90 and rate % nominal == 0]
    return max(matches, default=None)


class Control:
    def __init__(self):
        self.socket = socket.socket(socket.AF_UNIX)
        self.socket.settimeout(2)
        self.socket.connect(os.path.join(os.environ['XDG_RUNTIME_DIR'], 'gamescope-0'))
        self.buffer = b''
        self.internal = False
        self.rates = []
        self.refresh_only = False
        self.bound = False
        self.send(1, 1, words(2))  # wl_display.get_registry
        deadline = time.monotonic() + 2
        while time.monotonic() < deadline:
            self.receive()
            if self.bound and self.rates and self.refresh_only:
                return
        raise OSError('No compatible Gamescope display control')

    def send(self, obj, opcode, payload):
        self.socket.sendall(words(obj, ((len(payload) + 8) << 16) | opcode) + payload)

    def receive(self):
        data = self.socket.recv(65536)
        if not data:
            raise EOFError
        self.buffer += data
        while len(self.buffer) >= 8:
            obj, header = struct.unpack_from('=II', self.buffer)
            size, opcode = header >> 16, header & 65535
            if size < 8 or size % 4:
                raise ValueError('Invalid Wayland message')
            if len(self.buffer) < size:
                break
            body, self.buffer = self.buffer[8:size], self.buffer[size:]
            if obj == 1 and opcode == 0:
                raise OSError('Gamescope protocol error')
            if obj == 2 and opcode == 0 and not self.bound:
                name, = struct.unpack_from('=I', body)
                interface, offset = field(body, 4)
                version, = struct.unpack_from('=I', body, offset)
                if interface == b'gamescope_control\0' and version >= 2:
                    self.send(2, 0, words(name) + string('gamescope_control') + words(2, 3))
                    self.bound = True
            if obj == 3 and opcode == 0:
                feature, version, _flags = struct.unpack('=III', body)
                if feature == 4 and version >= 1:
                    self.refresh_only = True
            if obj == 3 and opcode == 1:
                offset = 0
                for _ in range(3):
                    _, offset = field(body, offset)
                flags, = struct.unpack_from('=I', body, offset)
                rates, _ = field(body, offset + 4)
                self.internal = bool(flags & 1)
                self.rates = list(struct.unpack('=' + 'I' * (len(rates) // 4), rates))

    def set(self, fps, flags=7):
        # internal_display | allow_refresh_switching | only_change_refresh_rate
        self.send(3, 1, words(fps, flags))


def settings():
    names = ['GAMESCOPE_DISPLAY_REFRESH_RATE_FEEDBACK', 'GAMESCOPE_LIMITER_FEEDBACK',
             'GAMESCOPE_FPS_LIMIT', 'GAMESCOPE_DYNAMIC_REFRESH']
    result = subprocess.run(['xprop', '-display', ':0', '-root', *names],
                            capture_output=True, text=True, timeout=2, check=True)
    values = {}
    for line in result.stdout.splitlines():
        if '=' in line:
            name, value = line.split('=', 1)
            values[name.split('(')[0]] = int(value.strip())
    return tuple(values[n] for n in names)


def foreground():
    app = os.environ.get('SteamAppId', '')
    if not app.isdecimal():
        return True  # standalone fixture/desktop launch
    result = subprocess.run(['xprop', '-display', ':0', '-root', 'GAMESCOPE_FOCUSED_APP'],
                            capture_output=True, text=True, timeout=2, check=True)
    focused = int(result.stdout.rsplit('=', 1)[1].strip())
    return focused in (int(app), 0, 769)  # allow Steam's transient overlay


def main(path):
    stop = False
    def stopping(_signal, _frame):
        nonlocal stop
        stop = True
    signal.signal(signal.SIGTERM, stopping)
    signal.signal(signal.SIGINT, stopping)
    control = Control()
    if not control.internal:
        return
    previous = settings()
    mpv = socket.socket(socket.AF_UNIX)
    mpv.settimeout(2)
    mpv.connect(path)
    mpv.sendall((json.dumps({'command': ['observe_property', 1, 'container-fps']}) + '\n').encode())
    pending = b''
    target = None
    requested = None
    owned = False
    checked = time.monotonic()
    try:
        while not stop:
            ready, _, _ = select.select([mpv, control.socket], [], [], 1)
            if control.socket in ready:
                control.receive()
                if not control.internal:
                    break
            if mpv in ready:
                data = mpv.recv(65536)
                if not data:
                    break
                pending += data
                if len(pending) > 65536:
                    raise ValueError('Oversized IPC message')
                while b'\n' in pending:
                    line, pending = pending.split(b'\n', 1)
                    event = json.loads(line)
                    if event.get('name') != 'container-fps':
                        continue
                    requested = cadence(event.get('data'), control.rates)
            if requested and requested != target and foreground():
                previous = settings()
                control.set(requested)
                target, owned = requested, True
                checked = time.monotonic() + 3  # mode-switch grace period
            if owned and time.monotonic() - checked >= 2:
                if not foreground():
                    break  # release the lease before another game's playback
                current = settings()
                if current[0] != target or current[1] != 0:
                    owned = False  # a user/Steam change supersedes our lease
                    break
                checked = time.monotonic()
    finally:
        if owned:
            current = settings()
            if not control.internal or (current[0] == target and current[1] == 0):
                hz, limited, cap, dynamic = previous
                control.set(cap if limited else (dynamic or hz), 3 if limited else 7)
        mpv.close()
        control.socket.close()


if __name__ == '__main__':
    try:
        main(sys.argv[1])
    except (OSError, EOFError, ValueError, KeyError, struct.error, subprocess.SubprocessError):
        # Optional platform feature: playback still works if unavailable.
        pass
