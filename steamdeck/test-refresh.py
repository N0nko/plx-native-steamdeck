"""Cadence policy and optional exclusive Gamescope lease integration test."""
import math
import os
import pathlib
import socket
import subprocess
import sys
import tempfile
import time
import unittest

import refresh


class CadenceTests(unittest.TestCase):
    def test_fractional_and_integer_film_rates(self):
        rates = [48, 50, 60, 72, 90]
        for fps, wanted in [(23.976, 72), (24, 72), (25, 50), (29.97, 90), (50, 50), (59.94, 60)]:
            self.assertEqual(refresh.cadence(fps, rates), wanted)

    def test_never_invent_a_display_mode(self):
        self.assertEqual(refresh.cadence(24, [40, 48, 60]), 48)
        self.assertIsNone(refresh.cadence(24, [60]))
        self.assertIsNone(refresh.cadence(24, []))

    def test_unknown_or_variable_rates_do_not_change_display(self):
        for fps in [None, '24', 0, -24, 23.5, math.nan, math.inf]:
            self.assertIsNone(refresh.cadence(fps, [48, 72]))

    def test_wire_strings_are_aligned_and_round_trip(self):
        for value in ['x', 'gamescope_control', 'abcd']:
            wire = refresh.string(value)
            data, end = refresh.field(wire, 0)
            self.assertEqual(data, value.encode() + b'\0')
            self.assertEqual(end, len(wire))
            self.assertEqual(len(wire) % 4, 0)
        with self.assertRaises(ValueError):
            refresh.field(refresh.words(10) + b'x', 0)


def device_test():
    assert subprocess.run(['pgrep', '-x', 'mpv-bin'], stdout=subprocess.DEVNULL).returncode == 1
    prior = refresh.settings()
    control = refresh.Control()
    assert control.internal and 72 in control.rates
    with tempfile.TemporaryDirectory(dir=os.environ['XDG_RUNTIME_DIR']) as folder:
        path = pathlib.Path(folder) / 'fixture.sock'
        server = socket.socket(socket.AF_UNIX)
        server.bind(str(path))
        server.listen(1)
        server.settimeout(5)
        child = subprocess.Popen([sys.executable, '-I', str(pathlib.Path(refresh.__file__)), str(path)])
        connection = None
        try:
            connection, _ = server.accept()
            assert b'container-fps' in connection.recv(4096)
            connection.sendall(b'{"event":"property-change","name":"container-fps","data":23.976}\n')
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and refresh.settings()[:2] != (72, 0):
                time.sleep(.1)
            assert refresh.settings()[:2] == (72, 0), refresh.settings()
            connection.close()
            connection = None
            child.wait(5)
            deadline = time.monotonic() + 5
            while time.monotonic() < deadline and refresh.settings()[:2] != prior[:2]:
                time.sleep(.1)
            assert refresh.settings()[:2] == prior[:2], refresh.settings()
            print('PASS internal panel matched 23.976 fps to 72 Hz without limiter; IPC close restored previous display', flush=True)
        finally:
            if connection:
                connection.close()
            server.close()
            if child.poll() is None:
                child.terminate()
                child.wait(5)
            hz, limited, cap, dynamic = prior
            control.set(cap if limited else (dynamic or hz), 3 if limited else 7)
            control.socket.close()


if __name__ == '__main__':
    if '--device' in sys.argv:
        device_test()
    else:
        unittest.main()
