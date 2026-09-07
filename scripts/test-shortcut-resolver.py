#!/usr/bin/env python3
"""Check the embedded key resolver from a relocated, standalone executable."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--binary', type=Path, required=True)
args = parser.parse_args()
with tempfile.TemporaryDirectory(prefix="just-speak resolver '$' ") as directory:
    root = Path(directory)
    binary = root / 'just-speak'
    shutil.copyfile(args.binary.resolve(), binary)
    binary.chmod(0o755)
    config = root / 'config/just-speak'
    config.mkdir(parents=True)
    (config / 'config.toml').write_text('this is intentionally invalid TOML')
    gjs = root / 'gjs'
    gjs.write_text('''#!/usr/bin/python3
import json, os, pathlib, sys
args = sys.argv[1:]
assert args[0] == '-c' and len(args) == 4, args
assert 'function resolveKey' in args[1] and 'Gtk.init()' in args[1]
assert args[2:] == ['95', '16777274'], args[2:]
pathlib.Path(os.environ['RESOLVER_TEST_LOG']).write_text(json.dumps(args[2:]))
print('embedded helper rejected key', file=sys.stderr) if os.environ.get('RESOLVER_TEST_FAIL') else print('{"key":"F11"}')
sys.exit(19 if os.environ.get('RESOLVER_TEST_FAIL') else 0)
''')
    gjs.chmod(0o755)
    log = root / 'invoked.json'
    env = dict(os.environ, HOME=str(root), XDG_CONFIG_HOME=str(root / 'config'),
               PATH=str(root), RESOLVER_TEST_LOG=str(log))

    def run(code, key, failure=False):
        return subprocess.run([str(binary), 'shortcut', 'resolve-key', str(code), str(key)],
                              env=dict(env, RESOLVER_TEST_FAIL='1' if failure else ''),
                              text=True, capture_output=True, timeout=5)

    result = run(95, 16777274)
    assert result.returncode == 0 and json.loads(result.stdout) == {'key': 'F11'}, result
    assert json.loads(log.read_text()) == ['95', '16777274']
    result = run(95, 16777274, failure=True)
    assert result.returncode == 19 and 'embedded helper rejected key' in result.stderr, result
    log.unlink()
    for code, key in [(7, 16777274), (65536, 16777274), ('95;echo', 16777274), (95, -1)]:
        assert run(code, key).returncode != 0
        assert not log.exists(), 'Invalid input reached the helper'
print('PASS: standalone embedded resolver, invalid settings, literal arguments, exit status, input bounds')
