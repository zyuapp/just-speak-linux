#!/usr/bin/env python3
"""Exercise the real daemon/model with synthetic desktop and microphone helpers.

Requires a built binary and the downloaded model's test_wavs/0.wav. Everything
written lives in a temporary directory; PATH contains only our fake helpers.
No real microphone, compositor, clipboard, or desktop configuration is accessed.
"""

import argparse
import json
import os
from pathlib import Path
import signal
import subprocess
import sys
import tempfile
import time
import wave


HELPER = r'''
import json, os, shutil, signal, sys, time
from pathlib import Path
root = Path(os.environ["JUST_SPEAK_SMOKE_ROOT"])
name = Path(sys.argv[0]).name
def event(kind):
    fd = os.open(root / "events.jsonl", os.O_WRONLY | os.O_CREAT | os.O_APPEND, 0o600)
    try:
        os.write(fd, (json.dumps({"event": kind, "pid": os.getpid()}) + "\n").encode())
    finally:
        os.close(fd)
if name == "pw-record":
    args = sys.argv[1:]
    for option, value in [("--rate", "16000"), ("--channels", "1"), ("--format", "s16")]:
        assert args[args.index(option) + 1] == value
    signal.signal(signal.SIGINT, lambda *_: sys.exit(0))
    shutil.copyfile((root / "fixture-path").read_text(), args[-1])
    event("recording_started")
    try:
        while True:
            signal.pause()
    finally:
        event("recording_stopped")
elif name == "wl-copy":
    text = sys.stdin.read()
    event("copy_started")
    while (root / "hold-clipboard").exists():
        time.sleep(0.01)
    (root / "clipboard.txt").write_text(text)
    event("copy_finished")
elif name == "hyprctl":
    if sys.argv[1:] == ["-j", "activewindow"]:
        print(json.dumps({"address": "0x1234", "class": "firefox", "initialClass": "firefox", "pid": 42, "tags": []}))
    elif sys.argv[1] == "eval":
        script = sys.argv[2]
        assert "JUST_SPEAK_FOCUS_CHANGED" in script and "send_key_state" in script
        assert (root / "clipboard.txt").read_text().strip()
        event("dispatch")
        print("ok")
    else:
        sys.exit("unexpected fake hyprctl command")
else:
    sys.exit("unknown fake helper")
'''


def wait_for(predicate, description, timeout=45):
    deadline = time.monotonic() + timeout
    while time.monotonic() < deadline:
        if result := predicate():
            return result
        time.sleep(0.03)
    raise AssertionError(f"timed out waiting for {description}")


def read_jsonl(path):
    if not path.exists():
        return []
    # A watcher can be in the middle of writing its next JSON line.
    return [json.loads(line) for line in path.read_text().splitlines(keepends=True)
            if line.endswith("\n") and line.strip()]


def stop_process(process):
    if process and process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=45)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait()


def smoke(binary, model, root):
    fixture = model / "test_wavs/0.wav"
    assert fixture.is_file(), f"missing speech fixture: {fixture}"
    for directory in ["bin", "config/just-speak", "runtime", "data", "cache", "tmp"]:
        (root / directory).mkdir(parents=True, mode=0o700)
    for program in ["pw-record", "wl-copy", "hyprctl"]:
        helper = root / "bin" / program
        helper.write_text(f"#!{sys.executable}\n" + HELPER)
        helper.chmod(0o700)
    (root / "fixture-path").write_text(str(fixture))
    # Extend the supplied speech fixture so cancellation lands during inference,
    # including on fast CPUs, while staying below the 120-second audio limit.
    long_fixture = root / "long.wav"
    with wave.open(str(fixture)) as source:
        params, frames = source.getparams(), source.readframes(source.getnframes())
    duration = params.nframes / params.framerate
    repetitions = max(1, int(40 / duration))
    with wave.open(str(long_fixture), "wb") as output:
        output.setparams(params)
        output.writeframes(frames * repetitions)
    (root / "config/just-speak/config.toml").write_text(
        f"model_dir = {json.dumps(str(model))}\nnum_threads = 4\npaste = true\n"
    )
    environment = dict(os.environ, PATH=str(root / "bin"), TMPDIR=str(root / "tmp"),
                       XDG_CONFIG_HOME=str(root / "config"), XDG_RUNTIME_DIR=str(root / "runtime"),
                       XDG_DATA_HOME=str(root / "data"), XDG_CACHE_HOME=str(root / "cache"),
                       JUST_SPEAK_SMOKE_ROOT=str(root))
    for name in ["WAYLAND_DISPLAY", "DISPLAY", "HYPRLAND_INSTANCE_SIGNATURE"]:
        environment.pop(name, None)
    socket = root / "runtime/just-speak/control.sock"
    daemon_log, watch_log = root / "daemon.log", root / "watch.jsonl"
    daemon = watcher = None

    def cli(*args, check=True):
        result = subprocess.run([str(binary), *args], env=environment, capture_output=True,
                                text=True, timeout=8)
        if check:
            assert result.returncode == 0, f"{' '.join(args)} failed: {result.stderr}"
        return result

    def status():
        assert daemon.poll() is None, "daemon exited unexpectedly"
        return json.loads(cli("status", "--json").stdout)

    def idle():
        current = status()
        assert current["phase"] != "error", current
        return current["phase"] == "idle" and current["model_ready"]

    def count(kind):
        return sum(event["event"] == kind for event in read_jsonl(root / "events.jsonl"))

    def clean_recordings():
        return not list((root / "tmp").glob("just-speak-recording-*"))

    try:
        with daemon_log.open("w") as logs, watch_log.open("w") as states:
            daemon = subprocess.Popen([str(binary), "daemon"], env=environment,
                                      stdout=logs, stderr=logs)
            wait_for(socket.exists, "daemon socket", timeout=10)
            watcher = subprocess.Popen([str(binary), "watch"], env=environment,
                                       stdout=states, stderr=logs)
            wait_for(lambda: read_jsonl(watch_log), "initial watch status")
            assert read_jsonl(watch_log)[0]["phase"] == "loading"
            wait_for(idle, "model loading to finish")
            duplicate = cli("daemon", check=False)
            assert duplicate.returncode != 0 and "already running" in duplicate.stderr
            assert idle() and socket.exists()
            print("PASS model loading, watch initial state, and single-instance protection", flush=True)

            cli("start")
            cli("start")
            assert status()["phase"] == "recording"
            assert count("recording_started") == 1, "duplicate start created a second recorder"
            cli("stop")
            wait_for(idle, "first dictation delivery")
            assert count("dispatch") == 1
            assert "old portrait" in (root / "clipboard.txt").read_text().lower()
            wait_for(clean_recordings, "finished audio cleanup")
            print("PASS recording → real transcription → clipboard → simulated paste", flush=True)

            cli("start")
            settings = root / "config/just-speak/config.toml"
            valid_settings = settings.read_text()
            settings.write_text("invalid TOML while dictating = [")
            cli("cancel")
            cli("stop")
            cli("cancel")
            wait_for(idle, "recording cancellation")
            settings.write_text(valid_settings)
            wait_for(clean_recordings, "canceled audio cleanup")
            assert count("dispatch") == 1 and count("copy_started") == 1
            print("PASS cancellation/status survive invalid edited config; idle stop/cancel are harmless", flush=True)

            (root / "fixture-path").write_text(str(long_fixture))
            cli("start")
            cli("stop")
            time.sleep(0.1)
            assert status()["phase"] == "transcribing"
            cli("cancel")
            assert idle()
            wait_for(clean_recordings, "canceled inference to release its audio")
            assert count("dispatch") == 1 and count("copy_started") == 1
            (root / "fixture-path").write_text(str(fixture))
            print("PASS canceled in-flight inference never copies or pastes", flush=True)

            gate = root / "hold-clipboard"
            gate.touch()
            cli("start")
            cli("stop")
            wait_for(lambda: count("copy_started") == 2, "delayed clipboard handshake")
            started = time.monotonic()
            assert status()["phase"] == "transcribing"
            cli("cancel")
            assert time.monotonic() - started < 1.0, "clipboard helper blocked status/cancel"
            assert idle()
            gate.unlink()
            wait_for(lambda: count("copy_finished") == 2, "clipboard handshake completion")
            time.sleep(0.15)
            assert count("dispatch") == 1 and idle()
            print("PASS cancellation stays responsive during clipboard handshake and suppresses paste", flush=True)

            cli("start")
            assert status()["phase"] == "recording"
            daemon.send_signal(signal.SIGTERM)
            assert daemon.wait(timeout=15) == 0
            assert not socket.exists() and clean_recordings(), "shutdown left socket or audio behind"
            assert count("recording_started") == count("recording_stopped")
            phases = [entry["phase"] for entry in read_jsonl(watch_log)]
            expected = iter(["loading", "idle", "recording", "transcribing", "idle"])
            next_phase = next(expected)
            for phase in phases:
                if phase == next_phase:
                    next_phase = next(expected, None)
            assert next_phase is None, f"missing watch transitions: {phases}"
            assert count("dispatch") == 1, "canceled work injected a late paste"
            print("PASS watch transitions and graceful shutdown cleanup", flush=True)
    except Exception:
        if daemon_log.exists():
            print(daemon_log.read_text(), file=sys.stderr)
        raise
    finally:
        (root / "hold-clipboard").unlink(missing_ok=True)
        stop_process(watcher)
        stop_process(daemon)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=Path("target/release/just-speak"))
    parser.add_argument("--model-dir", type=Path, required=True)
    args = parser.parse_args()
    binary, model = args.binary.resolve(), args.model_dir.resolve()
    assert binary.is_file(), f"build the release binary first: {binary}"
    with tempfile.TemporaryDirectory(prefix="just-speak-smoke-") as temporary:
        smoke(binary, model, Path(temporary))
    print("All daemon smoke checks passed; no real desktop or microphone was accessed.")


if __name__ == "__main__":
    main()
