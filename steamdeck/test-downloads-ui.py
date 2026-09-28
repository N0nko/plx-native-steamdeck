"""Native Download menu -> original file -> disconnected server -> native HDR playback."""
import hashlib, json, os, pathlib, socket, subprocess, sys, time, uuid
root = pathlib.Path.home()/'.local/share/plxnative-steamdeck'
sys.path.insert(0, str(root/'assets'))
import mock_pms
run = root/'test-ui'/uuid.uuid4().hex
run.mkdir(parents=True, mode=0o700)
fixture = root/'fixtures/hdr10-av.mkv'
server, pms = mock_pms.serve(32499, extra_media=[fixture])
item = pms.lib.items[990001]
item.update(viewOffset=0, viewCount=0)
pms.lib.items = {990001:item}
env = dict(os.environ, DISPLAY=':1', XDG_RUNTIME_DIR='/run/user/1000', SDL_VIDEODRIVER='x11',
           LD_LIBRARY_PATH=str(root/'assets'), PLXNATIVE_RUNTIME_DIR=str(run),
           PLXNATIVE_APP_DIR=str(root/'assets'), PLXNATIVE_MPV_TEST_LOG='1')
for flag, value in [('token','synthetic-token'),('grid','0,0'),('itemmenu',''),('noidle',''),('deck-status','')]:
    (run/f'plxnative-{flag}').write_text(value)
def until(fn, seconds=20):
    end=time.monotonic()+seconds
    while time.monotonic()<end:
        result=fn()
        if result: return result
        time.sleep(.1)
    raise AssertionError('Timed out: '+str(fn))
def status():
    try: return json.loads((run/'deck-status.json').read_text())
    except (OSError, ValueError): return {}
with open(run/'app.log','w') as log:
    app=subprocess.Popen([str(root/'bin/plxnative-sim'),'127.0.0.1','32499'],env=env,stdout=log,stderr=log)
    try:
        events=run/'plxnative-events.log'
        until(lambda: events.exists() and 'overlay=itemmenu' in events.read_text())
        def key(token):
            with open(run/'plxnative-remote','w') as fifo: fifo.write(token+'\n')
            time.sleep(.3)
        # Go to Movie -> Mark Watched -> Download original. No watch action is activated.
        key('down'); key('down'); key('ok')
        def completed():
            for path in (run/'downloads').glob('*.json'):
                try:
                    entry=json.loads(path.read_text())
                    if entry['state']=='complete': return path.with_suffix('.media')
                except (OSError, ValueError): pass
            return None
        media=until(completed)
        assert hashlib.sha256(media.read_bytes()).digest()==hashlib.sha256(fixture.read_bytes()).digest()
        print('PASS: native Download original saved byte-identical HDR file')
        key('shot')
        server.shutdown(); server.server_close()
        time.sleep(.7)
        key('ok')
        until(lambda: status().get('hdr') and status().get('overlays',0)>0)
        current=status()
        assert current['hardware']=='vaapi' and not current['failed'],current
        print('PASS: native offline playback, VAAPI, HDR10 and PlxNative controls',current)
        ipc=socket.socket(socket.AF_UNIX); ipc.connect(f'/run/user/1000/plxnative-video-{app.pid}/mpv-{app.pid}.sock')
        ipc.sendall((json.dumps({'command':['script-message','plx-key','ok']})+'\n').encode())
        until(lambda: status().get('paused'))
        ipc.sendall((json.dumps({'command':['script-message','plx-click','640','630']})+'\n').encode())
        time.sleep(.6)
        ipc.sendall((json.dumps({'command':['script-message','plx-key','right']})+'\n').encode())
        time.sleep(.6)
        ipc.sendall((json.dumps({'command':['script-message','plx-key','right']})+'\n').encode())
        until(lambda: status().get('position',0)>2)
        assert status()['paused'], 'Seeking while paused must stay paused'
        print('PASS: native scrubber seeks the downloaded HDR file while paused')
        ipc.sendall((json.dumps({'command':['screenshot-to-file',f'/run/user/1000/plxnative-video-{app.pid}/downloaded.png','window']})+'\n').encode())
        time.sleep(.3)
        ipc.sendall((json.dumps({'command':['script-message','plx-key','back']})+'\n').encode())
        time.sleep(1)
        print('evidence:',run, 'player pid:',app.pid)
        app.terminate(); app.wait(3)
        (run/'deck-status.json').unlink(missing_ok=True)
        events.unlink(missing_ok=True)
        env['PLXNATIVE_START_DOWNLOADS']='1'
        app=subprocess.Popen([str(root/'bin/plxnative-sim'),'127.0.0.1','32499'],env=env,stdout=log,stderr=log)
        until(lambda: events.exists() and 'route=downloads' in events.read_text())
        key('ok')
        until(lambda: status().get('hdr') and status().get('overlays',0)>0)
        print('PASS: cold launch into Downloads and HDR playback with the server still stopped')
        ipc.close(); ipc=socket.socket(socket.AF_UNIX)
        ipc.connect(f'/run/user/1000/plxnative-video-{app.pid}/mpv-{app.pid}.sock')
        ipc.sendall((json.dumps({'command':['script-message','plx-key','back']})+'\n').encode())
        time.sleep(1)
        key('down'); key('ok')
        until(lambda: not media.exists())
        print('PASS: native Delete download removed the original file')
    finally:
        app.terminate()
        try: app.wait(3)
        except subprocess.TimeoutExpired: app.kill(); app.wait()
        server.shutdown(); server.server_close()
