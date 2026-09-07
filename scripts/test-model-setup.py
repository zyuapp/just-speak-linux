#!/usr/bin/env python3
"""Check first-run setup using an isolated daemon and synthetic downloads.

No network, microphone, clipboard, user service, or desktop settings are used.
An optional existing model verifies automatic loading after a simulated download.
"""

import argparse
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time


DOWNLOADER = r'''
import os, pathlib, sys, time
root = pathlib.Path(os.environ['JUST_SPEAK_SETUP_TEST_ROOT'])
script = sys.stdin.read()
assert 'sha256sum --check' in script
assert sys.argv[1:3] == ['-s', '--']
destination = pathlib.Path(sys.argv[3])
progress = pathlib.Path(os.environ['JUST_SPEAK_MODEL_PROGRESS_FILE'])
attempt = int((root / 'attempt').read_text()) + 1 if (root / 'attempt').exists() else 1
(root / 'attempt').write_text(str(attempt))
progress.write_text('downloading\n')
while (root / 'hold').exists():
    time.sleep(0.02)
if (root / 'fail').exists():
    sys.exit('Synthetic connection failure. Please retry.')
for stage in ['verifying', 'extracting']:
    progress.write_text(stage + '\n')
    time.sleep(0.25)
# Transport/extraction is synthetic here. The normal inference loader validates
# and opens these real graphs, so reaching ready exercises the actual worker.
source = pathlib.Path((root / 'model-source').read_text())
destination.mkdir(parents=True)
for name in ['encoder.int8.onnx', 'decoder.int8.onnx', 'joiner.int8.onnx', 'tokens.txt']:
    (destination / name).symlink_to(source / name)
'''


def wait_for(check, description, timeout=30):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        result = check()
        if result:
            return result
        time.sleep(0.05)
    raise AssertionError(f'Timed out waiting for {description}')


def check_setup(binary, model, root):
    for name in ['bin', 'runtime', 'config', 'data', 'state', 'cache', 'tmp']:
        (root / name).mkdir(mode=0o700)
    downloader = root / 'bin/bash'
    downloader.write_text(f'#!{sys.executable}\n' + DOWNLOADER)
    downloader.chmod(0o700)
    env = dict(os.environ, PATH=str(root / 'bin'), TMPDIR=str(root / 'tmp'),
               XDG_RUNTIME_DIR=str(root / 'runtime'), XDG_CONFIG_HOME=str(root / 'config'),
               XDG_DATA_HOME=str(root / 'data'), XDG_STATE_HOME=str(root / 'state'),
               XDG_CACHE_HOME=str(root / 'cache'), JUST_SPEAK_SETUP_TEST_ROOT=str(root))
    for name in ['WAYLAND_DISPLAY', 'DISPLAY', 'HYPRLAND_INSTANCE_SIGNATURE']:
        env.pop(name, None)
    destination = root / 'model'
    config = root / 'config/just-speak/config.toml'
    config.parent.mkdir()
    config.write_text(f'model_dir = {json.dumps(str(destination))}\nauto_check_updates = false\n')

    def cli(*args, ok=True):
        result = subprocess.run([str(binary), *args], env=env, capture_output=True, text=True, timeout=8)
        assert (result.returncode == 0) == ok, (args, result.stdout, result.stderr)
        return result

    with (root / 'daemon.log').open('w+') as log:
        daemon = subprocess.Popen([str(binary), 'daemon'], env=env, stdout=log, stderr=log)
        try:
            wait_for(lambda: (root / 'runtime/just-speak/control.sock').exists(), 'daemon socket')

            def status():
                assert daemon.poll() is None, 'daemon exited unexpectedly'
                return json.loads(cli('status', '--json').stdout)

            wait_for(lambda: status().get('model_setup') == 'required', 'missing-model setup')
            assert not (root / 'attempt').exists(), 'first run must not start a download'
            cli('start', ok=False)
            assert not status()['model_ready']
            for attempt in [1, 2]:
                (root / 'hold').touch()
                (root / 'fail').touch()
                cli('model', 'setup')
                wait_for(lambda: (root / 'attempt').exists()
                         and (root / 'attempt').read_text() == str(attempt), 'download start')
                assert status()['phase'] == 'loading'
                assert status()['model_setup'] == 'downloading'
                # Each CLI connection has already closed; the service still
                # owns the download. A second client observes the same progress.
                cli('model', 'setup', ok=False)
                cli('start', ok=False)
                cli('restart', ok=False)
                cli('settings', 'set', 'history_enabled', 'false', ok=False)
                (root / 'hold').unlink()
                failed = wait_for(lambda: (state := status()).get('model_setup') == 'failed' and state,
                                  'download failure')
                assert 'Synthetic connection failure' in failed['message']
                assert not destination.exists()
            print('PASS missing model, explicit download, background ownership, busy gates, failure, retry')

            if model:
                (root / 'fail').unlink()
                (root / 'model-source').write_text(str(model))
                cli('model', 'setup')
                seen = set()

                def ready():
                    state = status()
                    seen.add(state.get('model_setup'))
                    assert state['phase'] != 'error', state
                    return state['model_ready'] and state['phase'] == 'idle'

                wait_for(ready, 'automatic model loading')
                assert {'verifying', 'extracting', 'loading'} <= seen, seen
                assert status().get('model_setup') is None
                cli('model', 'setup', ok=False)
                print('PASS automatic loading of a real model without restarting the daemon')
            cli('quit')
            daemon.wait(timeout=10)
        except BaseException:
            log.seek(0)
            print(log.read(), file=sys.stderr)
            raise
        finally:
            if daemon.poll() is None:
                daemon.terminate()
                daemon.wait(timeout=10)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/just-speak'))
    parser.add_argument('--model-dir', type=Path)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='just-speak-model-setup-') as temporary:
        check_setup(args.binary.resolve(), args.model_dir.resolve() if args.model_dir else None, Path(temporary))
