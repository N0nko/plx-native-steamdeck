"""Synthetic HDR playback workload; never touches a signed-in user's library."""
import json, os, pathlib, socket, subprocess, time, uuid

root = pathlib.Path.home()/'.local/share/plxnative-steamdeck'
run = root/'test-ui'/uuid.uuid4().hex
run.mkdir(parents=True, mode=0o700)
env = dict(os.environ, DISPLAY=':1', XDG_RUNTIME_DIR='/run/user/1000', SDL_VIDEODRIVER='x11',
           LD_LIBRARY_PATH=str(root/'assets'), PLXNATIVE_RUNTIME_DIR=str(run),
           PLXNATIVE_APP_DIR=str(root/'assets'), PLXNATIVE_MPV_TEST_LOG='1')
(run/'plxnative-token').write_text('synthetic-token')
(run/'plxnative-play').write_text('990001')
(run/'plxnative-deck-status').touch()

def ticks(pid):
    fields = pathlib.Path(f'/proc/{pid}/stat').read_text().split(') ')[1].split()
    return int(fields[11])+int(fields[12])

with open(run/'mock.log','w') as log, open(run/'app.log','w') as app_log:
    mock = subprocess.Popen(['python3', str(root/'assets/mock_pms.py'), '--port','32499', '--extra-media',str(root/'fixtures/hdr10-av.mkv')], stdout=log, stderr=log)
    app = None
    try:
        time.sleep(1)
        assert mock.poll() is None
        app = subprocess.Popen([str(root/'bin/plxnative-sim'),'127.0.0.1','32499'], env=env, stdout=app_log, stderr=app_log)
        deadline = time.monotonic()+30
        while time.monotonic()<deadline:
            try:
                status=json.loads((run/'deck-status.json').read_text())
                if status['ready']: break
            except (OSError,ValueError): pass
            assert app.poll() is None
            time.sleep(.05)
        else: raise AssertionError('Playback did not start')
        ipc=socket.socket(socket.AF_UNIX); ipc.settimeout(5)
        ipc.connect(f'/run/user/1000/plxnative-video-{app.pid}/mpv-{app.pid}.sock')
        channel=ipc.makefile('rwb',buffering=0)
        def command(*args):
            channel.write((json.dumps({'command':args,'request_id':42})+'\n').encode())
            while True:
                response=json.loads(channel.readline())
                if response.get('request_id')==42: return response.get('data')
        command('set_property','loop-file','inf')
        # The dev play trigger pins its HUD for captures. UP dismisses it through
        # the real native control path, so this measures actual film-only playback.
        command('script-message','plx-key','up')
        time.sleep(.2)
        command('script-message','plx-key','up')
        time.sleep(10)
        before=ticks(app.pid); started=time.monotonic()
        overlays=json.loads((run/'deck-status.json').read_text())['overlays']
        time.sleep(10)
        seconds=time.monotonic()-started
        status=json.loads((run/'deck-status.json').read_text())
        result={'ui_cpu_percent':round((ticks(app.pid)-before)/os.sysconf('SC_CLK_TCK')/seconds*100,2),
                'hud_uploads_per_second':round((status['overlays']-overlays)/seconds,2),
                'hardware':status['hardware'],'hdr':status['hdr'],
                'decoder_drops':command('get_property','decoder-frame-drop-count'),
                'output_drops':command('get_property','frame-drop-count')}
        command('screenshot-to-file',f'/run/user/1000/plxnative-video-{app.pid}/performance.png','window')
        print(json.dumps(result),flush=True)
        print('evidence:',run,flush=True)
        assert result['hud_uploads_per_second'] <= 5, 'Hidden HUD must not upload continuously'
        assert result['decoder_drops'] == 0 and result['output_drops'] == 0, 'Synthetic playback dropped frames'
    finally:
        if app:
            app.terminate()
            try: app.wait(4)
            except subprocess.TimeoutExpired: app.kill(); app.wait()
        mock.terminate(); mock.wait()
