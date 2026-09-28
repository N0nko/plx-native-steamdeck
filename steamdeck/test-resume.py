"""Resume metadata must reach the real Deck player, through a private mock PMS."""
import json, os, pathlib, socket, subprocess, sys, time, uuid

root = pathlib.Path.home() / '.local/share/plxnative-steamdeck'
sys.path.insert(0, str(root / 'assets'))
import mock_pms

def until(fn, seconds=25):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        result = fn()
        if result:
            return result
        time.sleep(.05)
    raise AssertionError('Timed out waiting for fixture playback')

assert subprocess.run(['pgrep', '-x', 'mpv-bin'], stdout=subprocess.DEVNULL).returncode == 1, 'Stop playback before this exclusive device test'
server, pms = mock_pms.serve(32499, extra_media=[root / 'fixtures/hdr4k23976-long.mkv'])
item = pms.lib.items[990001]
pms.lib.items = {990001: item}
try:
    for offset in (20_000, 0):
        item.update(viewOffset=offset, viewCount=0)
        run = root / 'test-ui' / uuid.uuid4().hex
        run.mkdir(parents=True, mode=0o700)
        for name, value in [('token', 'synthetic-token'), ('play', '990001'), ('deck-status', '')]:
            (run / f'plxnative-{name}').write_text(value)
        env = dict(os.environ, DISPLAY=':1', XDG_RUNTIME_DIR='/run/user/1000', SDL_VIDEODRIVER='x11',
                   LD_LIBRARY_PATH=str(root / 'assets'), PLXNATIVE_RUNTIME_DIR=str(run),
                   PLXNATIVE_APP_DIR=str(root / 'assets'))
        env.pop('PLXNATIVE_MPV_TEST_LOG', None)
        with (run / 'app.log').open('w') as log:
            app = subprocess.Popen([str(root / 'bin/plxnative-sim'), '127.0.0.1', '32499'], env=env, stdout=log, stderr=log)
            try:
                def ready():
                    assert app.poll() is None, 'Fixture app exited'
                    try:
                        status = json.loads((run / 'deck-status.json').read_text())
                        return status if status['ready'] else None
                    except (OSError, ValueError):
                        return None
                status = until(ready)
                expected = offset / 1000
                print('First presented position:', status['position'], 'expected:', expected, 'evidence:', run, flush=True)
                assert expected <= status['position'] < expected + 2, 'Saved Plex offset was lost before playback'
                assert not status['failed'], status
                print('PASS: resume' if offset else 'PASS: play from beginning', flush=True)
            finally:
                app.terminate()
                try:
                    app.wait(4)
                except subprocess.TimeoutExpired:
                    app.kill(); app.wait()
                until(lambda: subprocess.run(['pgrep', '-x', 'mpv-bin'], stdout=subprocess.DEVNULL).returncode == 1)
finally:
    server.shutdown(); server.server_close()
