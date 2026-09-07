#!/usr/bin/env python3
"""Run the real GTK window, CLI and daemon through desktop lifecycle workflows.

Uses a private session bus, temporary XDG directories, no speech model and fake
service-manager/download helpers. No recording, clipboard, user service, update
installation or desktop settings are touched. Briefly shows the test window.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import threading
import time


DRIVER = r'''
const testAction = new Gio.SimpleAction({name: 'workflow-test', parameter_type: new GLib.VariantType('s')});
testAction.connect('activate', (_action, value) => {
    try {
        const action = value.get_string()[0];
        if (action === 'close') window.close();
        else if (action !== 'snapshot') {
            const target = action === 'setup' ? controls.modelSetup.button : controls[action];
            if (!target?.sensitive) throw new Error(`Control unavailable: ${action}`);
            target.emit('clicked');
        }
        const result = {phase: status.phase, setup: status.model_setup, busy, visible: window.visible,
            status: controls.status.label, message: controls.message.label,
            start: controls.start.sensitive, quit: controls.quit.sensitive};
        GLib.file_set_contents(GLib.getenv('JUST_SPEAK_WINDOW_TEST_ROOT') + '/snapshot.json', JSON.stringify(result));
    } catch (error) {
        GLib.file_set_contents(GLib.getenv('JUST_SPEAK_WINDOW_TEST_ROOT') + '/driver-error', String(error));
    }
});
app.add_action(testAction);
'''

SERVICE_MANAGER = r'''
import json, os, pathlib, sys, time, uuid
root = pathlib.Path(os.environ['JUST_SPEAK_WINDOW_TEST_ROOT'])
assert sys.argv[1] == '--user' and sys.argv[3] == 'just-speak.service', sys.argv
name = uuid.uuid4().hex
request = root / 'requests' / (name + '.request')
temporary = request.with_suffix('.tmp')
temporary.write_text(sys.argv[2]); temporary.rename(request)
reply = request.with_suffix('.reply')
deadline = time.monotonic() + 8
while not reply.exists():
    if time.monotonic() > deadline: sys.exit('Test service manager timed out')
    time.sleep(0.01)
result = json.loads(reply.read_text())
if result.get('error'): sys.exit(result['error'])
'''

DOWNLOADER = r'''
import os, pathlib, signal, sys, time
root = pathlib.Path(os.environ['JUST_SPEAK_WINDOW_TEST_ROOT'])
sys.stdin.read()
signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
pathlib.Path(os.environ['JUST_SPEAK_MODEL_PROGRESS_FILE']).write_text('downloading\n')
(root / 'download-started').write_text(str(os.getpid()))
try:
    while True: time.sleep(0.02)
finally:
    (root / 'download-cleaned').touch()
'''


def wait_for(check, message, timeout=15):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if result := check():
            return result
        time.sleep(0.03)
    raise AssertionError(message)


def run(binary, root, backend):
    project = Path(__file__).resolve().parent.parent
    for name in ['bin', 'runtime', 'config/just-speak', 'data', 'state', 'cache', 'tmp', 'requests', 'prefix']:
        (root / name).mkdir(parents=True, mode=0o700)
    shutil.copytree(project / 'gtk', root / 'gtk')
    main = root / 'gtk/main.js'
    source = main.read_text()
    assert source.count('const exitCode = app.run(') == 1
    source = source.replace('const exitCode = app.run(', DRIVER + '\nconst exitCode = app.run(')
    source = source.replace("title: 'JustSpeak'", "title: 'JustSpeak — workflow test'")
    main.write_text(source)
    (root / 'config/just-speak/config.toml').write_text(
        f'model_dir = {json.dumps(str(root / "missing-model"))}\n'
        'auto_check_updates = false\nsound_feedback = false\nmute_while_recording = false\n')
    for name, script in {'systemctl': SERVICE_MANAGER, 'bash': DOWNLOADER,
                         'pw-dump': "print('[]')"}.items():
        helper = root / 'bin' / name
        helper.write_text(f'#!{sys.executable}\n' + script)
        helper.chmod(0o700)
    runtime = os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')
    wayland = os.environ.get('WAYLAND_DISPLAY', 'wayland-1')
    env = dict(os.environ, PATH=str(root / 'bin') + ':/usr/bin',
               JUST_SPEAK_BIN=str(binary), JUST_SPEAK_PREFIX=str(root / 'prefix'),
               JUST_SPEAK_WINDOW_TEST_ROOT=str(root),
               XDG_RUNTIME_DIR=str(root / 'runtime'), XDG_CONFIG_HOME=str(root / 'config'),
               XDG_DATA_HOME=str(root / 'data'), XDG_STATE_HOME=str(root / 'state'),
               XDG_CACHE_HOME=str(root / 'cache'), TMPDIR=str(root / 'tmp'),
               WAYLAND_DISPLAY=wayland if wayland.startswith('/') else runtime + '/' + wayland,
               GTK_A11Y='none', GIO_USE_VFS='local', GTK_USE_PORTAL='0', GDK_BACKEND=backend)
    env.pop('HYPRLAND_INSTANCE_SIGNATURE', None)
    socket_path = root / 'runtime/just-speak/control.sock'
    stop = threading.Event()
    daemons = []
    apps = []

    with (root / 'daemon.log').open('w+') as daemon_log, (root / 'window.log').open('w+') as window_log:
        def service_manager():
            while not stop.wait(0.01):
                for request in (root / 'requests').glob('*.request'):
                    reply = request.with_suffix('.reply')
                    if reply.exists():
                        continue
                    result = {}
                    try:
                        action = request.read_text()
                        assert action in ['start', 'restart'], action
                        daemon = daemons[-1] if daemons else None
                        if action == 'restart' and daemon and daemon.poll() is None:
                            daemon.terminate(); daemon.wait(timeout=10)
                        if daemon is None or daemon.poll() is not None:
                            daemon = subprocess.Popen([str(binary), 'daemon'], env=env,
                                                      stdin=subprocess.DEVNULL, stdout=daemon_log, stderr=daemon_log)
                            daemons.append(daemon)
                        wait_for(lambda: socket_path.exists(), 'Test daemon did not start', timeout=5)
                    except Exception as error:
                        result['error'] = str(error)
                    temporary = reply.with_suffix('.tmp')
                    temporary.write_text(json.dumps(result)); temporary.rename(reply)

        manager = threading.Thread(target=service_manager, daemon=True)
        manager.start()

        def action(name='snapshot'):
            snapshot = root / 'snapshot.json'
            snapshot.unlink(missing_ok=True)
            subprocess.run(['/usr/bin/gapplication', 'action', 'io.github.zyuapp.JustSpeak',
                            'workflow-test', json.dumps(name)], env=env, capture_output=True, check=True, timeout=4)
            if (root / 'driver-error').exists():
                raise AssertionError((root / 'driver-error').read_text())
            if name in ['close', 'quit']:
                return None
            wait_for(snapshot.exists, 'Window did not report its state')
            return json.loads(snapshot.read_text())

        def opened():
            app = subprocess.Popen(['/usr/bin/gjs', '-m', str(main)], env=env,
                                   stdout=window_log, stderr=window_log)
            apps.append(app)

            def registered():
                assert app.poll() is None, 'Window exited during startup'
                try:
                    return action()
                except subprocess.CalledProcessError:
                    return False

            wait_for(registered, 'Window did not register its actions')
            return app

        try:
            app = opened()
            ready = wait_for(lambda: (state := action()).get('setup') == 'required' and not state['busy'] and state,
                             'Opening window did not start model setup service')
            assert not ready['start'] and ready['quit'], ready
            first_pid = daemons[-1].pid
            action('close')
            assert app.wait(timeout=5) == 0
            assert daemons[-1].poll() is None and socket_path.exists(), 'Close unexpectedly stopped dictation service'
            app = opened()
            wait_for(lambda: action().get('setup') == 'required', 'Reopened window lost setup state')
            assert daemons[-1].pid == first_pid
            action('restart')
            wait_for(lambda: daemons[-1].pid != first_pid and action().get('setup') == 'required', 'Restart did not replace the daemon')
            subprocess.run([str(binary), 'quit'], env=env, capture_output=True, check=True, timeout=35)
            assert app.wait(timeout=5) == 0, 'CLI/bar Quit left the window open'
            assert daemons[-1].wait(timeout=1) == 0 and not socket_path.exists()
            print('PASS opening starts service, close preserves it, reopen reconnects, restart and external Quit close correctly', flush=True)

            app = opened()
            wait_for(lambda: (state := action()).get('setup') == 'required' and not state['busy'], 'Relaunch did not restart service')
            action('setup')
            wait_for(lambda: (root / 'download-started').exists() and action()['phase'] == 'loading', 'Setup did not begin')
            pid = daemons[-1].pid
            action('close')
            assert app.wait(timeout=5) == 0 and daemons[-1].poll() is None
            app = opened()
            wait_for(lambda: action()['phase'] == 'loading', 'Reopen lost running download')
            assert daemons[-1].pid == pid
            action('quit')
            assert app.wait(timeout=35) == 0, 'Quit button left the window open'
            assert daemons[-1].wait(timeout=1) == 0 and not socket_path.exists()
            assert (root / 'download-cleaned').exists(), 'Quit did not stop download'
            print('PASS setup outlives window close, reopening restores progress, real Quit button completes CLI/service cleanup', flush=True)
        except BaseException:
            for log in [daemon_log, window_log]:
                log.flush(); log.seek(0); print(log.read(), file=sys.stderr)
            raise
        finally:
            stop.set(); manager.join(timeout=6)
            for process in apps + daemons:
                if process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=10)
                    except subprocess.TimeoutExpired:
                        process.kill(); process.wait()


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, default=Path('target/debug/just-speak'))
    parser.add_argument('--backend', choices=['wayland', 'x11'], default='wayland')
    args = parser.parse_args()
    if os.environ.get('JUST_SPEAK_WINDOW_TEST_BUS') != 'private':
        env = dict(os.environ, JUST_SPEAK_WINDOW_TEST_BUS='private')
        raise SystemExit(subprocess.call(['dbus-run-session', '--', sys.executable,
                                         str(Path(__file__).resolve()), *sys.argv[1:]], env=env))
    with tempfile.TemporaryDirectory(prefix='just-speak-window-workflow-') as temporary:
        run(args.binary.resolve(), Path(temporary), args.backend)
