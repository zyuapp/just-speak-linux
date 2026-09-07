#!/usr/bin/env python3
"""Exercise real CLI shutdown using isolated daemons, downloads and window IPC.

Uses no model, network, desktop services, microphone, clipboard or user settings.
"""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time


def wait_for(check, message, timeout=8):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if result := check():
            return result
        time.sleep(0.02)
    raise AssertionError(message)


def test(binary, root):
    for name in ['bin', 'runtime', 'config', 'data', 'state', 'cache', 'tmp']:
        (root / name).mkdir(mode=0o700)
    env = dict(os.environ, PATH=str(root / 'bin'), TMPDIR=str(root / 'tmp'),
               XDG_RUNTIME_DIR=str(root / 'runtime'), XDG_CONFIG_HOME=str(root / 'config'),
               XDG_DATA_HOME=str(root / 'data'), XDG_STATE_HOME=str(root / 'state'),
               XDG_CACHE_HOME=str(root / 'cache'), JUST_SPEAK_LIFECYCLE_TEST=str(root))
    for name in ['WAYLAND_DISPLAY', 'DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE', 'DBUS_SESSION_BUS_ADDRESS']:
        env.pop(name, None)
    config = root / 'config/just-speak/config.toml'
    config.parent.mkdir()
    config.write_text(f'model_dir = {json.dumps(str(root / "missing-model"))}\nauto_check_updates = false\n')
    helpers = {
        'gapplication': '''import os, pathlib, sys
root = pathlib.Path(os.environ['JUST_SPEAK_LIFECYCLE_TEST'])
assert sys.argv[1:] == ['action', 'io.github.zyuapp.JustSpeak', 'quit']
assert not (root / 'runtime/just-speak/control.sock').exists(), 'window closed before shutdown'
with (root / 'window-closed').open('a') as log: log.write('closed\\n')
''',
        'bash': '''import os, pathlib, signal, sys, time
root = pathlib.Path(os.environ['JUST_SPEAK_LIFECYCLE_TEST'])
sys.stdin.read()
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
(root / 'download-started').touch()
try:
    while True: time.sleep(0.02)
finally:
    (root / 'download-cleaned').touch()
''',
    }
    for name, source in helpers.items():
        helper = root / 'bin' / name
        helper.write_text(f'#!{sys.executable}\n' + source)
        helper.chmod(0o700)

    def cli(*args, ok=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=35)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        return result

    # Already stopped must still close an open settings window, and invalid
    # preferences must never prevent Quit from controlling a running daemon.
    cli('quit')
    for attempt in range(12):
        config.write_text(f'model_dir = {json.dumps(str(root / "missing-model"))}\nauto_check_updates = false\n')
        with (root / 'daemon.log').open('w+') as log, (root / 'watch.log').open('w+') as events:
            daemon = subprocess.Popen([str(binary), 'daemon'], env=env, stdout=log, stderr=log)
            watcher = None
            partial_client = None
            try:
                socket_path = root / 'runtime/just-speak/control.sock'
                wait_for(socket_path.exists, 'socket never appeared')
                wait_for(lambda: json.loads(cli('status', '--json').stdout).get('model_setup') == 'required',
                         'missing-model state never appeared')
                watcher = subprocess.Popen([str(binary), 'watch'], env=env, stdout=events, stderr=subprocess.DEVNULL)
                wait_for(lambda: (root / 'watch.log').stat().st_size > 0, 'watcher did not subscribe')
                if attempt == 0:
                    # Use the real IPC gate so this also tests that a rejected
                    # Quit cannot send the cross-window close action.
                    def request(command):
                        with socket.socket(socket.AF_UNIX) as stream:
                            stream.connect(str(socket_path))
                            stream.sendall(json.dumps({'command': command}).encode() + b'\n')
                            return json.loads(stream.makefile().readline())

                    assert request('begin_update')['ok']
                    before = (root / 'window-closed').read_text()
                    assert 'update' in cli('quit', ok=False).stderr
                    assert daemon.poll() is None and before == (root / 'window-closed').read_text()
                    assert request('end_update')['ok']
                    cli('model', 'setup')
                    wait_for(lambda: (root / 'download-started').exists(), 'download did not start')
                    assert json.loads(cli('status', '--json').stdout)['phase'] == 'loading'
                if attempt == 1:
                    # A client that never finishes its request must not keep the
                    # old service alive after Quit reports success to a launcher.
                    partial_client = socket.socket(socket.AF_UNIX)
                    partial_client.connect(str(socket_path))
                    partial_client.sendall(b'{"command":')
                config.write_text('broken TOML = [')
                started = time.monotonic()
                cli('quit')
                assert not socket_path.exists(), 'Quit returned before the socket was removed'
                assert daemon.wait(timeout=1) == 0, 'Quit succeeded while the service process was still running'
                if partial_client:
                    assert time.monotonic() - started < 2, 'Partial client delayed shutdown'
                watcher.wait(timeout=5)
                phases = [json.loads(line)['phase'] for line in (root / 'watch.log').read_text().splitlines()]
                assert 'stopping' in phases, phases
                if attempt == 0:
                    assert (root / 'download-cleaned').exists(), 'Quit did not clean up the download'
            except BaseException:
                log.seek(0)
                print(log.read(), file=sys.stderr)
                raise
            finally:
                if partial_client:
                    partial_client.close()
                for process in [watcher, daemon]:
                    if process and process.poll() is None:
                        process.terminate()
                        process.wait(timeout=10)
    assert len((root / 'window-closed').read_text().splitlines()) == 13
    print('PASS Quit: stopped app, invalid config, rejected update, download cancellation, window close, slow client, 12 complete replies and watcher disconnects')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/just-speak'))
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='just-speak-lifecycle-') as temporary:
        test(args.binary.resolve(), Path(temporary))
