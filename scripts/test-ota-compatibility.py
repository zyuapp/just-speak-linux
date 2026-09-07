#!/usr/bin/env python3
"""Exercise a published old updater against the exact incoming release archive.

Usage: test-ota-compatibility.py PREVIOUS_RELEASE_DIRECTORY NEW_RELEASE_DIRECTORY
All installation, download, service and launcher effects stay in a temporary tree.
"""
import hashlib
import json
import os
import pathlib
import shutil
import subprocess
import sys
import tarfile
import tempfile


def verified_archive(directory):
    directory = pathlib.Path(directory).resolve()
    matches = list(directory.glob('just-speak-linux-v*-linux-x86_64.tar.gz'))
    assert len(matches) == 1, matches
    archive = matches[0]
    checksums = [line.split() for line in (directory / 'SHA256SUMS').read_text().splitlines()]
    expected = [digest for digest, name in checksums if name == archive.name]
    assert expected == [hashlib.sha256(archive.read_bytes()).hexdigest()], archive
    with tarfile.open(archive) as package:
        version = json.load(package.extractfile('release.json'))['version']
    return archive, version


old_archive, old_version = verified_archive(sys.argv[1])
new_archive, new_version = verified_archive(sys.argv[2])
with tempfile.TemporaryDirectory(prefix='just-speak-ota-test-') as temporary:
    root = pathlib.Path(temporary)
    prefix = root / 'prefix with spaces'
    helpers = root / 'helpers'
    helpers.mkdir()
    fixtures = root / 'downloads'
    fixtures.mkdir()
    repository = 'zyuapp/just-speak-linux'
    base = f'https://github.com/{repository}/releases/download/v{new_version}/'
    metadata = {
        'tag_name': 'v' + new_version,
        'html_url': f'https://github.com/{repository}/releases/tag/v{new_version}',
        'draft': False, 'prerelease': False,
        'assets': [
            {'name': name, 'browser_download_url': base + name,
             'size': (new_archive.parent / name).stat().st_size}
            for name in (new_archive.name, 'SHA256SUMS')
        ],
    }
    (fixtures / 'latest').write_text(json.dumps(metadata))
    for name in (new_archive.name, 'SHA256SUMS'):
        shutil.copyfile(new_archive.parent / name, fixtures / name)

    def helper(name, source):
        path = helpers / name
        path.write_text(source)
        path.chmod(0o755)

    helper('curl', f'''#!{sys.executable}
import pathlib, shutil, sys
source = pathlib.Path({str(fixtures)!r}) / sys.argv[-1].rsplit('/', 1)[-1]
shutil.copyfile(source, sys.argv[sys.argv.index('--output') + 1])
''')
    helper('systemctl', '#!/bin/sh\nexit 3\n')
    helper('gjs', '#!/bin/sh\nexit 0\n')
    env = dict(os.environ)
    for name in ('HYPRLAND_INSTANCE_SIGNATURE', 'WAYLAND_DISPLAY', 'DBUS_SESSION_BUS_ADDRESS'):
        env.pop(name, None)
    env.update({
        'HOME': str(root / 'home'), 'JUST_SPEAK_PREFIX': str(prefix),
        'XDG_CONFIG_HOME': str(root / 'config'), 'XDG_DATA_HOME': str(root / 'data'),
        'XDG_STATE_HOME': str(root / 'state'), 'XDG_RUNTIME_DIR': str(root / 'runtime'),
        'PATH': str(helpers) + os.pathsep + os.environ['PATH'],
    })
    for name in ('HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME', 'XDG_RUNTIME_DIR'):
        pathlib.Path(env[name]).mkdir()
    bootstrap = root / 'old-updater'
    with tarfile.open(old_archive) as package:
        bootstrap.write_bytes(package.extractfile('bin/just-speak').read())
    bootstrap.chmod(0o755)

    def run(*args):
        return subprocess.check_output([str(arg) for arg in args], env=env, text=True)

    assert run(bootstrap, '--version').strip() == 'just-speak ' + old_version
    run(bootstrap, 'update', 'install-archive', old_archive, '--version', old_version)
    installed = prefix / 'bin/just-speak'
    current = prefix / 'share/just-speak/updates/current'
    old_target = current.resolve()
    # Existing user files must survive the real old updater's atomic switch.
    sentinel = pathlib.Path(env['XDG_DATA_HOME']) / 'keep-model-and-history'
    sentinel.write_text('preserve me')
    assert json.loads(run(installed, 'update', 'check', '--json'))['available']
    result = json.loads(run(installed, 'update', 'apply'))
    assert result['previous_version'] == old_version, result
    assert result['installed_version'] == new_version, result
    assert run(installed, '--version').strip() == 'just-speak ' + new_version
    assert current.resolve() != old_target
    assert (current.parent / 'previous').resolve() == old_target
    assert sentinel.read_text() == 'preserve me'
    assert not json.loads(run(installed, 'update', 'check', '--json'))['available']
    # Opening the installed window repairs the pre-icon launcher without a reinstall.
    data = pathlib.Path(env['XDG_DATA_HOME'])
    launcher = data / 'applications/just-speak.desktop'
    launcher.parent.mkdir()
    launcher.write_text('[Desktop Entry]\nExec=keep-this\nIcon=audio-input-microphone\n')
    run(installed, 'window')
    icon = data / 'icons/hicolor/scalable/apps/just-speak.svg'
    assert icon.read_text() == run(installed, 'export-icon')
    assert launcher.read_text() == '[Desktop Entry]\nExec=keep-this\nIcon=just-speak\n'
    print(f'Published v{old_version} updater installed v{new_version}; rollback, user data, '
          'update check and launcher icon verified in an isolated installation.')
