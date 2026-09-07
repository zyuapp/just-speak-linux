#!/usr/bin/env python3
"""Exercise the installed Omarchy panel with isolated settings and a fake backend.

--compositor briefly maps a test panel and uses real shortcut inhibition/keymap
lookup. No user bindings, microphone, clipboard or services are changed.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--compositor', action='store_true')
parser.add_argument('--failure', choices=['denied', 'lookup-timeout', 'malformed', 'capture-timeout'])
parser.add_argument('--keep', type=Path, help='Keep fixture and logs here')
args = parser.parse_args()
project = Path(__file__).resolve().parent.parent
temporary = tempfile.TemporaryDirectory(prefix='just-speak-inline-')
root = args.keep.resolve() if args.keep else Path(temporary.name)
root.mkdir(parents=True, exist_ok=True)
for name in ['home', 'config', 'cache', 'runtime', 'bin', 'Commons']:
    (root / name).mkdir(exist_ok=True)
(root / 'runtime').chmod(0o700)
shell = Path('/usr/share/omarchy/shell')
shutil.copytree(shell / 'Ui', root / 'Ui', dirs_exist_ok=True)
for name in ['Style.qml', 'Color.qml', 'Border.qml', 'BorderGeometry.js', 'Util.qml']:
    shutil.copyfile(shell / 'Commons' / name, root / 'Commons' / name)
(root / 'Commons/qmldir').write_text('module qs.Commons\n' + ''.join(
    f'singleton {name} 1.0 {name}.qml\n' for name in ['Style', 'Color', 'Border', 'Util']))
shutil.copytree(project / 'ui', root / 'App', dirs_exist_ok=True)
shutil.copytree(project / 'gtk', root / 'gtk', dirs_exist_ok=True)
shutil.copyfile(project / 'ui/tests/popup.qml', root / 'shell.qml')
(root / 'bin/hyprctl').write_text('#!/bin/sh\nprintf \'{"int":5}\\n\'\n')
(root / 'bin/hyprctl').chmod(0o755)
backend = root / 'bin/just-speak-fake'
backend.write_text('''#!/usr/bin/python3
import json, os, pathlib, sys, time
root = pathlib.Path(os.environ['HARNESS_ROOT'])
args = sys.argv[1:]
with (root / 'commands.jsonl').open('a') as log: log.write(json.dumps(args) + '\\n')
if args == ['watch']:
    print(json.dumps({'phase':'idle', 'model_ready':True, 'shortcut':'CTRL + F11'}), flush=True)
    time.sleep(60)
elif args == ['menu', '--json']:
    saved = root / 'saved'
    print(json.dumps({'settings':{'shortcut':saved.read_text() if saved.exists() else 'CTRL + F11', 'auto_check_updates':False}, 'inputs':[], 'history':[], 'desktop':{'shortcut_editing':True}, 'version':'fixture'}))
elif args[:2] == ['shortcut', 'set']:
    time.sleep(0.15)
    if not (root / 'conflicted').exists():
        (root / 'conflicted').touch()
        sys.exit('Fixture conflict: shortcut is already assigned')
    (root / 'saved').write_text(args[2])
else:
    sys.exit('Unexpected action: ' + repr(args))
''')
backend.chmod(0o755)
for name in ['commands.jsonl', 'saved', 'conflicted', 'lookup-count']:
    (root / name).unlink(missing_ok=True)
if not args.compositor:
    (root / 'gtk/shortcut-keymap.js').write_text("""import GLib from 'gi://GLib';
const path = GLib.getenv('HARNESS_ROOT') + '/lookup-count';
const first = !GLib.file_test(path, GLib.FileTest.EXISTS);
GLib.file_set_contents(path, 'seen');
const failure = GLib.getenv('HARNESS_FAILURE');
if (first && failure === 'lookup-timeout') GLib.usleep(6000000);
print(first && failure === 'malformed' ? 'invalid JSON' : JSON.stringify({key: 'F10'}));
""")
runtime = os.environ.get('XDG_RUNTIME_DIR', f'/run/user/{os.getuid()}')
wayland = os.environ.get('WAYLAND_DISPLAY', 'wayland-1')
if not wayland.startswith('/'):
    wayland = runtime + '/' + wayland
env = dict(os.environ, HOME=str(root / 'home'), XDG_CONFIG_HOME=str(root / 'config'),
           XDG_CACHE_HOME=str(root / 'cache'), XDG_RUNTIME_DIR=str(root / 'runtime'),
           WAYLAND_DISPLAY=wayland, QT_QPA_PLATFORM='wayland',
           QT_QUICK_BACKEND='software', QT_QPA_PLATFORMTHEME='', QT_STYLE_OVERRIDE='Fusion',
           QT_QUICK_CONTROLS_STYLE='Basic', XDG_CURRENT_DESKTOP='', DESKTOP_SESSION='',
           JUST_SPEAK_BIN=str(backend), HARNESS_ROOT=str(root),
           HARNESS_COMPOSITOR='1' if args.compositor else '0', HARNESS_FAILURE=args.failure or '', PATH=str(root / 'bin') + ':/usr/bin')
for name in ['DISPLAY', 'DBUS_SESSION_BUS_ADDRESS', 'GNOME_DESKTOP_SESSION_ID']:
    env.pop(name, None)
result = subprocess.run(['quickshell', '--no-color', '--path', str(root / 'shell.qml')],
                        env=env, capture_output=True, text=True, timeout=60)
output = result.stdout + result.stderr
(root / 'output.log').write_text(output)
print(output)
assert result.returncode == 0 and 'INLINE_PASS' in output, f'Popup check failed: {root}'
assert not any(word in output for word in ['TypeError:', 'ReferenceError:', 'Binding loop', 'Unable to assign']), output
commands = [json.loads(line) for line in (root / 'commands.jsonl').read_text().splitlines()]
assert all(command in [['watch'], ['menu', '--json'], ['shortcut', 'set', 'F10']] for command in commands), commands
assert commands.count(['shortcut', 'set', 'F10']) == (0 if args.failure else 2), commands
print('PASS: ' + (args.failure if args.failure else 'inline UI, capability gates, conflict/retry, save routing, cancellation') + '; no window launch')
if args.keep:
    print(f'Fixture and logs: {root}')
