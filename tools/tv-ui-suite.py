#!/usr/bin/env python3
"""Batch real remote keys and collect a review gallery from a stable release.

No developer triggers, installer operations or hidden app state.
Authorized playback changes resume history; navigation plans avoid account mutation controls.
Plans are navigation fixtures, not assertions that pictures look correct.
SSH keys are preferred; SSHPASS optionally enables sshpass without storing a secret.
"""
import argparse
import hashlib
import html
import json
import os
from pathlib import Path
import re
import shlex
import struct
import subprocess
import tempfile
import time
import threading
from contextlib import contextmanager

KEYS = {"UP": 103, "DOWN": 108, "LEFT": 105, "RIGHT": 106,
        "OK": 28, "BACK": 1, **{str(i): i + 1 for i in range(1, 10)}, "0": 11}


def gallery(output, report):
    (output / "manifest.json").write_text(json.dumps(report, indent=2))
    cards = []
    for row in report["steps"]:
        name = html.escape(row["name"])
        cards.append(f'<article><h2>{name}</h2><p>{html.escape(row["observed"])} · '
                     f'{html.escape(row["status"])}</p><a href="{name}.png">'
                     f'<img loading="lazy" src="{name}.png"></a>'
                     f'<p>{html.escape(row.get("review", ""))}</p></article>')
    (output / "index.html").write_text(
        '<!doctype html><meta charset="utf-8"><title>PlxNative TV review</title>'
        '<style>body{background:#14161c;color:#eee;font:16px system-ui;margin:24px}'
        'main{display:grid;grid-template-columns:repeat(auto-fit,minmax(440px,1fr));gap:24px}'
        'img{width:100%}article{background:#23262e;padding:16px;border-radius:16px}'
        'h2{font-size:18px}p{color:#ccd}</style><h1>PlxNative TV review</h1>'
        f'<p>Build {html.escape(report["revision"])}. Images require visual review; '
        'route checks only establish navigation.</p><main>' + ''.join(cards) + '</main>')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--tv', required=True)
    parser.add_argument('--plan', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--allow-playback', action='store_true')
    parser.add_argument('--capture-service', default='com.webos.service.capture')
    args = parser.parse_args()
    plan = json.loads(args.plan.read_text())
    if plan.get('playback') and not args.allow_playback:
        parser.error('playback plan requires --allow-playback')
    root = Path(__file__).resolve().parent.parent
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    env = dict(os.environ, TV=args.tv)
    report = dict(revision=subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=root, text=True).strip(),
                  plan=plan, steps=[], completed=False)
    prefix = ['sshpass', '-e'] if os.environ.get('SSHPASS') else []
    with tempfile.TemporaryDirectory(prefix='plx-ui-') as tmp:
        ssh = prefix + ['ssh', '-o', 'StrictHostKeyChecking=accept-new', '-o', 'ConnectTimeout=5',
                        '-o', 'ControlMaster=auto', '-o', 'ControlPersist=30',
                        '-o', 'ControlPath=' + tmp + '/ssh', 'root@' + args.tv]

        def remote(command, data=None, tty=False):
            cmd = ssh[:-1] + (['-tt'] if tty else []) + [ssh[-1], command]
            return subprocess.run(cmd, input=data, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                                  check=True, timeout=20).stdout

        def state():
            lines = remote('tail -n 160 /tmp/plxnative-events.log').decode(errors='replace').splitlines()
            beats = [s for s in lines if re.match(r'^loop=\d+ route=', s)]
            if not beats:
                raise RuntimeError('No current application heartbeat')
            beat = beats[-1]
            route = re.search(r'route=(\w+)(?: overlay=(\w+))?', beat)
            return '/'.join(x for x in route.groups() if x), beats

        def keys(sequence, gap):
            # One authenticated channel per complete macro, not one SSH per key edge.
            proc = subprocess.Popen(ssh + ['/bin/cat > /dev/input/event1'], stdin=subprocess.PIPE,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.PIPE)
            try:
                for key in sequence:
                    for value in (1, 0):
                        proc.stdin.write(struct.pack(fmt, 0, 0, 1, KEYS[key], value) +
                                         struct.pack(fmt, 0, 0, 0, 0, 0))
                        proc.stdin.flush()
                        time.sleep(.08 if value else gap)
                proc.stdin.close()
                if proc.wait(timeout=10):
                    raise RuntimeError('Remote input channel failed')
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()

        @contextmanager
        def holding(key):
            if not key:
                yield
                return
            if key not in ('LEFT', 'RIGHT') or not args.allow_playback or state()[0] != 'player/none':
                raise RuntimeError('Held seek requires authorized active playback with no overlay')
            proc = subprocess.Popen(ssh + ['/bin/cat > /dev/input/event1'], stdin=subprocess.PIPE,
                                    stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            stop = threading.Event()
            errors = []
            def edge(value):
                proc.stdin.write(struct.pack(fmt, 0, 0, 1, KEYS[key], value) + struct.pack(fmt, 0, 0, 0, 0, 0))
                proc.stdin.flush()
            def repeat():
                try:
                    edge(1)
                    stop.wait(.35)
                    while not stop.is_set():
                        edge(2)
                        stop.wait(.1)
                except Exception as error:
                    errors.append(error)
                finally:
                    try:
                        edge(0)
                    except (BrokenPipeError, OSError) as error:
                        errors.append(error)
            thread = threading.Thread(target=repeat)
            thread.start()
            try:
                yield
            finally:
                stop.set()
                thread.join(timeout=3)
                proc.stdin.close()
                try:
                    proc.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    proc.kill()
                    proc.wait()
                if errors or proc.returncode:
                    raise RuntimeError('Held seek input failed; release was attempted')

        subprocess.run(['tools/tv-lock.sh', 'acquire', '--why', 'authorized batch UI/playback captures'],
                       cwd=root, env=env, check=True)
        try:
            assert remote('cat /sys/class/input/event1/device/name').strip() == b'LGE RCU'
            elf = remote('head -c 5 /bin/cat')
            assert elf[:4] == b'\x7fELF'
            fmt = '<qqHHi' if elf[4] == 2 else '<iiHHi'
            boot = remote('head -n 6 /tmp/plxnative-events.log').decode(errors='replace')
            assert 'features=release' in boot, 'Expected stable release startup'
            installed = remote('sha256sum /media/developer/apps/usr/palm/applications/com.beb.plxnative/plxnative').decode().split()[0]
            expected = hashlib.sha256((root / 'pkg/plxnative').read_bytes()).hexdigest()
            assert installed == expected, 'Installed binary differs from this checkout build'
            report['installed_sha256'] = installed
            for step in plan['steps']:
                name = step['name']
                assert re.fullmatch('[a-z0-9-]+', name)
                before, _ = state()
                if step.get('before') and not re.fullmatch(step['before'], before):
                    raise RuntimeError(f'{name}: expected {step["before"]}, found {before}; stopped before input')
                keys(step.get('keys', []), step.get('gap', .18))
                with holding(step.get('hold')):
                    time.sleep(step.get('settle', 1.3))
                    observed, beats = state()
                    path = '/tmp/plx-suite-' + name + '.png'
                    payload = json.dumps(dict(path=path, method='DISPLAY', width=1920, height=1080, format='PNG'))
                    response = remote('luna-send -n 1 luna://' + args.capture_service + '/executeOneShot ' + shlex.quote(payload), tty=True)
                    assert b'"returnValue":true' in response.replace(b' ', b''), 'Capture failed'
                    data = remote('cat ' + path)
                    assert data.startswith(b'\x89PNG'), 'Invalid screenshot'
                    (output / (name + '.png')).write_bytes(data)
                    remote('rm -f ' + path)
                matched = not step.get('after') or re.fullmatch(step['after'], observed)
                row = dict(step, observed=observed, status='route checked; visual review needed' if matched else 'ROUTE MISMATCH',
                           timestamp=time.time(), sha256=hashlib.sha256(data).hexdigest())
                report['steps'].append(row)
                (output / (name + '.log')).write_text('\n'.join(beats) + '\n')
                gallery(output, report)
                print(name + ': ' + observed, flush=True)
                if not matched:
                    raise RuntimeError(name + ': route mismatch; macro stopped')
            report['completed'] = True
        finally:
            gallery(output, report)
            subprocess.run(['tools/tv-lock.sh', 'release'], cwd=root, env=env, check=True)
            subprocess.run(ssh[:-1] + ['-O', 'exit', ssh[-1]], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


if __name__ == '__main__':
    main()
