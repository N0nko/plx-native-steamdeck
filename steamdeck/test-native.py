"""Run only against our generated fixture and mock PMS, in a fresh private app root."""
import json, os, pathlib, socket, subprocess, time, uuid
root = pathlib.Path.home()/'.local/share/plxnative-steamdeck'
run = root/'test-ui'/uuid.uuid4().hex
run.mkdir(parents=True, mode=0o700)
env = dict(os.environ, DISPLAY=':1', XDG_RUNTIME_DIR='/run/user/1000', SDL_VIDEODRIVER='x11',
           LD_LIBRARY_PATH=str(root/'assets'), PLXNATIVE_RUNTIME_DIR=str(run),
           PLXNATIVE_APP_DIR=str(root/'assets'), PLXNATIVE_SHOT=str(run/'native.png'),
           PLXNATIVE_SHOT_FRAME='300', PLXNATIVE_SHOT_ALPHA='1')
env['PLXNATIVE_MPV_TEST_LOG'] = str(root/'native-mpv-test.log')
(run/'plxnative-token').write_text('synthetic-token')
(run/'plxnative-play').write_text('990001')
(run/'plxnative-deck-status').touch()
with open(run/'mock.log','w') as log:
    mock = subprocess.Popen(['python3', str(root/'assets/mock_pms.py'), '--port','32499', '--extra-media',str(root/'fixtures/hdr10-av.mkv')], stdout=log, stderr=log)
    try:
        time.sleep(1)
        assert mock.poll() is None, 'Mock PMS startup failed'
        with open(run/'app.log','w') as app_log:
            app = subprocess.Popen([str(root/'bin/plxnative-sim'), '127.0.0.1','32499'], env=env, stdout=app_log, stderr=app_log)
            try:
                def wait_for(predicate, seconds=20):
                    deadline = time.monotonic() + seconds
                    while time.monotonic() < deadline:
                        try:
                            status = json.loads((run/'deck-status.json').read_text())
                            if predicate(status): return status
                        except (OSError, ValueError): pass
                        assert app.poll() is None, 'Native app exited'
                        time.sleep(.05)
                    raise AssertionError('Timed out waiting for native playback status')
                status = wait_for(lambda s: s['ready'] and s['overlays'] > 0)
                ipc = socket.socket(socket.AF_UNIX)
                ipc.connect(f'/run/user/1000/plxnative-video-{app.pid}/mpv-{app.pid}.sock')
                def command(*args): ipc.sendall((json.dumps({'command': args})+'\n').encode())
                command('script-message', 'plx-key', 'ok')
                status = wait_for(lambda s: s['paused'])
                print('PASS: native OK paused playback', status)
                command('screenshot-to-file', f'/run/user/1000/plxnative-video-{app.pid}/composited.png', 'window')
                time.sleep(.3)
                # The native scrubber receives right then OK; it owns the seek command.
                command('script-message', 'plx-key', 'right')
                time.sleep(.3)
                command('script-message', 'plx-key', 'ok')
                time.sleep(.5)
                command('script-message', 'plx-key', 'back')
                time.sleep(1)
                print('native process exit:', app.poll())
                print('evidence:', run)
            finally:
                app.terminate()
                try: app.wait(3)
                except subprocess.TimeoutExpired: app.kill(); app.wait()
    finally:
        mock.terminate(); mock.wait()
