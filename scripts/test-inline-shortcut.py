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
parser.add_argument('--plugin', type=Path, help='Use an installed plugin directory for live verification')
parser.add_argument('--binary', type=Path, help='JustSpeak binary for live key resolution')
parser.add_argument('--compositor', action='store_true')
parser.add_argument('--lifecycle', action='store_true', help='Check Quit failure/completion and bar visibility')
parser.add_argument('--workflows', action='store_true', help='Check menu errors, stale reads, timeouts and update retry')
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
# Match the installed plugin: QML is loaded through a symlink outside the
# payload directory, while the helper lives beside the payload's actual UI.
payload = root / 'payload/share/just-speak'
shutil.copytree(project / 'ui', payload / 'ui', dirs_exist_ok=True)
shutil.copytree(project / 'gtk', payload / 'gtk', dirs_exist_ok=True)
if not (root / 'App').is_symlink():
    (root / 'App').symlink_to(args.plugin.absolute() if args.plugin else payload / 'ui', target_is_directory=True)
fixture = 'workflows' if args.workflows else 'lifecycle' if args.lifecycle else 'popup'
shutil.copyfile(project / f'ui/tests/{fixture}.qml', root / 'shell.qml')
if args.workflows:
    # Exercise the real deadline handlers without spending a minute waiting.
    for name, replacements in {
        'MenuModel.qml': [('interval: 8000', 'interval: 500'),
                          ('args[0] === "quit" ? 35000 : args[0] === "shortcut" ? 25000 : 10000', '500')],
        'UpdateModel.qml': [('interval: 45000', 'interval: 500')],
        'CommandProcess.qml': [('interval: 1000', 'interval: 100')],
    }.items():
        path = payload / 'ui' / name
        source = path.read_text()
        for old, new in replacements:
            assert old in source, f'Missing fixture deadline: {name}: {old}'
            source = source.replace(old, new)
        path.write_text(source)
(root / 'bin/hyprctl').write_text('#!/bin/sh\nprintf \'{"int":5}\\n\'\n')
(root / 'bin/hyprctl').chmod(0o755)
backend = root / 'bin/just-speak-fake'
backend.write_text('''#!/usr/bin/python3
import json, os, pathlib, sys, time
root = pathlib.Path(os.environ['HARNESS_ROOT'])
args = sys.argv[1:]
with (root / 'commands.jsonl').open('a') as log: log.write(json.dumps(args) + '\\n')
if os.environ.get('HARNESS_WORKFLOWS') == '1':
    config_path = root / 'behavior.json'
    config = json.loads(config_path.read_text()) if config_path.exists() else {}
    preferences_path = root / 'preferences.json'
    preferences = json.loads(preferences_path.read_text()) if preferences_path.exists() else {'paste': False}
    if args[0] == 'fixture':
        config_path.write_text(args[1])
    elif args == ['watch']:
        print(json.dumps({'phase':'idle', 'model_ready':True, 'shortcut':'CTRL + F11'}), flush=True)
        time.sleep(60)
    elif args == ['menu', '--json']:
        time.sleep(config.get('menu_delay', 0))
        if config.get('menu_error'): sys.exit('Fixture: microphone enumeration failed')
        if config.get('menu_malformed'): print('invalid JSON')
        else:
            print(json.dumps({'settings':dict(preferences, shortcut='CTRL + F11', auto_check_updates=False),
                'inputs':[], 'history':[{'id':7, 'created_at':1700000000, 'text':'Fixture transcript'}],
                'desktop':{'shortcut_editing':True}, 'version':'fixture'}))
    elif args[:2] == ['settings', 'set']:
        time.sleep(0.1)
        preferences[args[2]] = args[3] == 'true'
        preferences_path.write_text(json.dumps(preferences))
    elif args == ['start']:
        time.sleep(0.1)
        if config.get('action_error'): sys.exit('Fixture: microphone is unavailable')
    elif args[:2] == ['history', 'paste']:
        time.sleep(0.1)
        if config.get('action_error'): sys.exit('Fixture: paste target disappeared')
    elif args[:2] == ['history', 'copy']:
        pass
    elif args == ['restart']:
        time.sleep(config.get('action_delay', 0))
    elif args == ['update', 'check', '--json']:
        time.sleep(config.get('check_delay', 0))
        if config.get('check_error'): sys.exit('Fixture: update server is unavailable')
        if config.get('check_malformed'): print(json.dumps({'available':True, 'latest_version':{}}))
        else: print(json.dumps({'available':True, 'latest_version':'9.9.9', 'release_url':''}))
    elif args == ['update', 'install']:
        time.sleep(0.15)
        if config.get('install_error'): sys.exit('Fixture: update download failed')
    else:
        sys.exit('Unexpected workflow action: ' + repr(args))
    sys.exit(0)
if args == ['watch']:
    if os.environ['HARNESS_LIFECYCLE'] == '1' and (root / 'stopped').exists(): sys.exit(1)
    print(json.dumps({'phase':'idle', 'model_ready':True, 'shortcut':'CTRL + F11'}), flush=True)
    if os.environ['HARNESS_LIFECYCLE'] == '1':
        while not (root / 'stopped').exists(): time.sleep(0.02)
    else: time.sleep(60)
elif args == ['menu', '--json']:
    if (root / 'stopped').exists(): sys.exit('Must not refresh a stopped service')
    saved = root / 'saved'
    print(json.dumps({'settings':{'shortcut':saved.read_text() if saved.exists() else 'CTRL + F11', 'auto_check_updates':False}, 'inputs':[], 'history':[], 'desktop':{'shortcut_editing':True}, 'version':'fixture'}))
elif args[:2] == ['shortcut', 'resolve-key']:
    if os.environ['HARNESS_COMPOSITOR'] == '1':
        binary = os.environ['HARNESS_BINARY']
        os.execv(binary, [binary] + args)
    os.execv('/usr/bin/gjs', ['gjs', '-m', str(root / 'payload/share/just-speak/gtk/shortcut-keymap.js')] + args[2:])
elif args[:2] == ['shortcut', 'set']:
    time.sleep(0.15)
    if not (root / 'conflicted').exists():
        (root / 'conflicted').touch()
        sys.exit('Fixture conflict: shortcut is already assigned')
    (root / 'saved').write_text(args[2])
elif os.environ['HARNESS_LIFECYCLE'] == '1' and args == ['quit']:
    time.sleep(0.3)
    if not (root / 'conflicted').exists():
        (root / 'conflicted').touch()
        sys.exit('Fixture: update is still running')
    (root / 'stopped').touch()
elif os.environ['HARNESS_LIFECYCLE'] == '1' and args == ['launch']:
    (root / 'stopped').unlink()
else:
    sys.exit('Unexpected action: ' + repr(args))
''')
backend.chmod(0o755)
for name in ['commands.jsonl', 'saved', 'conflicted', 'lookup-count', 'stopped', 'behavior.json', 'preferences.json']:
    (root / name).unlink(missing_ok=True)
if not args.compositor:
    (payload / 'gtk/shortcut-keymap.js').write_text("""import GLib from 'gi://GLib';
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
           HARNESS_BINARY=str((args.binary or project / 'target/debug/just-speak').resolve()),
           HARNESS_COMPOSITOR='1' if args.compositor else '0', HARNESS_FAILURE=args.failure or '', PATH=str(root / 'bin') + ':/usr/bin')
env['HARNESS_LIFECYCLE'] = '1' if args.lifecycle else '0'
env['HARNESS_WORKFLOWS'] = '1' if args.workflows else '0'
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
if args.workflows:
    commands = [command for command in commands if command[0] != 'fixture']
    assert all(command in [['watch'], ['menu', '--json'], ['start'], ['history', 'paste', '7'],
        ['history', 'copy', '7'], ['settings', 'set', 'paste', 'true'], ['restart'],
        ['update', 'check', '--json'], ['update', 'install']] for command in commands), commands
    assert commands.count(['start']) == 2 and commands.count(['history', 'paste', '7']) == 1, commands
    assert commands.count(['update', 'install']) == 2, commands
    print('PASS: menu action errors/retry, refresh serialization, settings/update timeout recovery, canceling gates')
    if args.keep: print(f'Fixture and logs: {root}')
    raise SystemExit(0)
if args.lifecycle:
    assert commands.count(['quit']) == 2 and commands.count(['launch']) == 1, commands
    assert all(command in [['watch'], ['menu', '--json'], ['quit'], ['launch']] for command in commands), commands
    last_quit = max(index for index, command in enumerate(commands) if command == ['quit'])
    launch = commands.index(['launch'])
    assert ['menu', '--json'] not in commands[last_quit + 1:launch], commands
    print('PASS: Quit waits for completion, preserves errors, hides bar icon and restores it on launch')
    if args.keep: print(f'Fixture and logs: {root}')
    raise SystemExit(0)
resolutions = [command for command in commands if command[:2] == ['shortcut', 'resolve-key']]
assert resolutions or args.failure in ['denied', 'capture-timeout'], commands
commands = [command for command in commands if command[:2] != ['shortcut', 'resolve-key']]
assert all(command in [['watch'], ['menu', '--json'], ['shortcut', 'set', 'F10'], ['shortcut', 'set', 'SUPER + F11']] for command in commands), commands
assert commands.count(['shortcut', 'set', 'F10']) == (0 if args.failure else 2), commands
assert commands.count(['shortcut', 'set', 'SUPER + F11']) == (1 if args.compositor and not args.failure else 0), commands
print('PASS: ' + (args.failure if args.failure else 'inline UI, capability gates, conflict/retry, save routing, cancellation') + '; no window launch')
if args.keep:
    print(f'Fixture and logs: {root}')
