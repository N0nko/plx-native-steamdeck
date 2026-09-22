#!/usr/bin/env python3
"""Offline host checks for the production software-audio C boundary.

Requires a C compiler, system ffmpeg (fixture encoding), SDL2 and a host build of the
bundled FFmpeg 9 with dca/truehd/flac/opus/vorbis decoders and libswresample enabled.
This does NOT compile Rust or establish television output, latency or A/V sync.
"""
from __future__ import annotations
import argparse
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parents[1]

def run(args: list[str], **kwargs) -> str:
    result = subprocess.run(args, text=True, stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                            timeout=90, **kwargs)
    if result.returncode:
        raise RuntimeError(f"Command failed ({result.returncode}): {args!r}\n{result.stderr}\n{result.stdout}")
    return result.stdout.strip()

def configure(lib: ctypes.CDLL) -> None:
    ptr = ctypes.c_void_p
    lib.plx_audio_init.argtypes = [ctypes.c_char_p] * 3
    lib.plx_audio_init.restype = ctypes.c_uint
    lib.plx_audio_decoder_open.argtypes = [ptr]
    lib.plx_audio_decoder_open.restype = ptr
    lib.plx_pcm_open.restype = ptr
    for name in ('close', 'flush'):
        getattr(lib, f'plx_pcm_{name}').argtypes = [ptr]
        getattr(lib, f'plx_pcm_{name}').restype = None
    lib.plx_pcm_pause.argtypes = [ptr, ctypes.c_int]
    lib.plx_pcm_pause.restype = None
    lib.plx_pcm_queued.argtypes = [ptr]
    lib.plx_pcm_queued.restype = ctypes.c_uint
    lib.plx_pcm_feed.argtypes = [ptr, ptr, ctypes.c_int, ctypes.c_int64, ctypes.c_int64, ctypes.c_int]
    lib.plx_pcm_feed.restype = ctypes.c_int

def pcm_checks(test_library: str, production_library: str, disabled_library: str) -> dict:
    os.environ['SDL_AUDIODRIVER'] = 'dummy'
    sdl = ctypes.util.find_library('SDL2-2.0') or ctypes.util.find_library('SDL2')
    if not sdl:
        raise RuntimeError('SDL2 runtime is required for deterministic queue tests')
    ctypes.CDLL(sdl, mode=ctypes.RTLD_GLOBAL)
    lib = ctypes.CDLL(test_library); configure(lib)
    count = 0
    sink = lib.plx_pcm_open()
    assert sink and lib.plx_pcm_queued(sink) == 0; count += 1
    data = (ctypes.c_uint8 * (65536*4))(*([1, 2, 3, 4] * 65536))
    try:
        for n in [-1, 0, 3, 48004, 65536*4+4]:
            assert lib.plx_pcm_feed(sink, data, n, 0, 0, 1) == -1
        assert lib.plx_pcm_feed(sink, None, 3840, 0, 0, 1) == -1
        assert lib.plx_pcm_feed(sink, data, 3840, 2**63-1, 0, 1) == -1
        count += 1
        assert lib.plx_pcm_feed(sink, data, 3840, 1_000_000_000, 0, 1) == 0
        assert lib.plx_pcm_queued(sink) == 0; count += 1
        assert lib.plx_pcm_feed(sink, data, 3840, 0, 0, 1) == 1
        assert lib.plx_pcm_queued(sink) == 3840; count += 1
        time.sleep(0.04)
        assert lib.plx_pcm_queued(sink) == 3840; count += 1
        for i in range(1, 12):
            assert lib.plx_pcm_feed(sink, data, 3840, i*20_000_000, 0, 1) == 1
        assert lib.plx_pcm_feed(sink, data, 3840, 240_000_000, 0, 1) == 0
        assert lib.plx_pcm_queued(sink) == 46080; count += 1
        lib.plx_pcm_flush(sink)
        assert lib.plx_pcm_queued(sink) == 0; count += 1
        assert lib.plx_pcm_feed(sink, data, 3840, 0, 1_000_000_000, 0) == 1
        assert lib.plx_pcm_queued(sink) == 0; count += 1
        assert lib.plx_pcm_feed(sink, data, 3840, -3600_000_000_000, 604800_000_000_000, 0) == 1
        assert lib.plx_pcm_queued(sink) == 0; count += 1  # bounded arithmetic for a distant timestamp
        assert lib.plx_pcm_feed(sink, data, 19200, 0, 90_000_000, 0) == 1
        assert lib.plx_pcm_queued(sink) == 1920; count += 1
        lib.plx_pcm_pause(sink, 0)
        deadline = time.monotonic() + 2
        while lib.plx_pcm_queued(sink) and time.monotonic() < deadline:
            time.sleep(0.01)
        assert lib.plx_pcm_queued(sink) == 0; count += 1
        lib.plx_pcm_flush(sink)
        assert lib.plx_pcm_feed(sink, data, 3840, 0, 0, 1) == 1
        time.sleep(0.04)
        assert lib.plx_pcm_queued(sink) == 3840; count += 1
    finally:
        lib.plx_pcm_close(sink)
    sink = lib.plx_pcm_open()
    assert sink and lib.plx_pcm_queued(sink) == 0
    lib.plx_pcm_close(sink); count += 1
    production = ctypes.CDLL(production_library); configure(production)
    assert not production.plx_pcm_open(); count += 1  # dummy must never qualify in shipping code
    disabled = ctypes.CDLL(disabled_library); configure(disabled)
    assert disabled.plx_audio_init(b'/missing', b'/missing', b'/missing') == 0
    assert not disabled.plx_audio_decoder_open(None) and not disabled.plx_pcm_open(); count += 1
    return {'passed': count, 'backend': 'SDL dummy (test-only)', 'hardware_sync_tested': False}

def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--ffmpeg-prefix', type=Path, required=True)
    parser.add_argument('--output', type=Path, required=True)
    parser.add_argument('--sanitize', action='store_true', help='Instrument the production C decoder wrapper with ASan/UBSan')
    args = parser.parse_args()
    prefix = args.ffmpeg_prefix.resolve(strict=True)
    libdir = prefix/'lib'
    def library(name: str, major: int) -> Path:
        for n in (f'lib{name}-plx.so.{major}', f'lib{name}.so.{major}'):
            if (libdir/n).is_file(): return libdir/n
        raise RuntimeError(f'Missing {name} ABI {major} in {libdir}')
    libraries = [library('avutil',61), library('avcodec',63), library('swresample',7)]
    avformat = library('avformat',63)
    cc = os.environ.get('CC','cc')
    result = {'scope':'production C boundary only; no Rust build or TV validation', 'decoder_cases':[],
              'sanitizers':args.sanitize}
    with tempfile.TemporaryDirectory(prefix='plx-audio-test-') as temp:
        tmp = Path(temp)
        includes = ['-Isrc', '-Iinclude', '-I'+str(prefix/'include')]
        common = [cc,'-std=gnu11','-Wall','-Wextra','-Werror', *includes]
        test = tmp/'test.so'; production=tmp/'production.so'; disabled=tmp/'disabled.so'
        for output, flags in [(test,['-DPLX_LOCAL_AUDIO=1','-DPLX_AUDIO_TEST_DUMMY=1']),
                              (production,['-DPLX_LOCAL_AUDIO=1']), (disabled,[])]:
            run(common+['-fPIC','-shared',*flags,'src/local_audio.c','src/local_pcm.c','-ldl','-o',str(output)],cwd=ROOT)
        exe=tmp/'probe'
        sanitize=['-fsanitize=address,undefined','-fno-omit-frame-pointer','-g'] if args.sanitize else []
        run(common+sanitize+['-DPLX_LOCAL_AUDIO=1','tests/local_audio_probe.c','src/local_audio.c',
            str(avformat),str(libraries[1]),str(libraries[0]),'-ldl','-Wl,-rpath,'+str(libdir),'-o',str(exe)],cwd=ROOT)
        env=dict(os.environ,LD_LIBRARY_PATH=str(libdir)+':'+os.environ.get('LD_LIBRARY_PATH',''))
        for encoder,rate,channels in [('dca',48000,2),('truehd',48000,2),('flac',48000,2),
                                       ('libopus',48000,2),('libvorbis',48000,2),('flac',44100,6)]:
            path=tmp/f'{encoder}-{rate}-{channels}.mka'
            run(['ffmpeg','-hide_banner','-loglevel','error','-y','-f','lavfi','-i',
                 f'sine=frequency=997:duration=1:sample_rate={rate}', '-ac',str(channels),
                 '-c:a',encoder,'-strict','-2',str(path)])
            for seek in (False,True):
                command=[str(exe),*[str(p) for p in libraries],str(path),str(tmp/'decoded.pcm')]
                if seek: command.append('seek-and-flush')
                observation=json.loads(run(command,env=env))
                result['decoder_cases'].append({'encoder':encoder,'rate':rate,'source_channels':channels,
                     'seek_flush_replay':seek,**observation})
        # SDL deterministic tests run without sanitizers; the decoder probe above owns the instrumented boundary.
        result['pcm']=pcm_checks(str(test),str(production),str(disabled))
    result['passed']=True
    args.output.parent.mkdir(parents=True,exist_ok=True)
    args.output.write_text(json.dumps(result,indent=2)+'\n',encoding='utf-8')
    print(json.dumps(result,indent=2))

if __name__=='__main__':
    main()
