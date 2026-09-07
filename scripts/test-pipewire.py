#!/usr/bin/env python3
"""Verify real pw-record WAV capture with a private, synthetic-only PipeWire server.

Requires PipeWire's audiotestsrc SPA plugin and pw-link. No hardware discovery,
session manager, physical microphone, audio playback, or user configuration is
used. The isolated server and every temporary file are removed after the test.
"""

import os
from pathlib import Path
import signal
import struct
import subprocess
import tempfile
import time
import wave


SERVER = """
context.properties = {
    core.daemon = true core.name = just-speak-test-0 support.dbus = false
    default.clock.rate = 16000 default.clock.quantum = 256
}
context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    audio.adapt = audioconvert/libspa-audioconvert
    support.* = support/libspa-support
    audiotestsrc = audiotestsrc/libspa-audiotestsrc
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-spa-node-factory }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-access }
    { name = libpipewire-module-adapter }
    { name = libpipewire-module-link-factory }
]
context.objects = [
    { factory = spa-node-factory args = {
        factory.name = support.node.driver node.name = TestDriver priority.driver = 20000
    } }
    { factory = spa-node-factory args = {
        factory.name = audiotestsrc node.name = JustSpeakTestSource media.class = Audio/Source
        node.param.Props = { live = true frequency = 440 volume = 0.2 }
    } }
]
"""
CLIENT = """
context.properties = { support.dbus = false }
context.spa-libs = {
    audio.convert.* = audioconvert/libspa-audioconvert
    support.* = support/libspa-support
}
context.modules = [
    { name = libpipewire-module-protocol-native }
    { name = libpipewire-module-client-node }
    { name = libpipewire-module-adapter }
]
stream.properties = { adapter.auto-port-config = { mode = passthrough } }
"""


def main():
    with tempfile.TemporaryDirectory(prefix="just-speak-pipewire-") as temporary:
        root = Path(temporary)
        (root / "testserver.conf").write_text(SERVER)
        (root / "client.conf").write_text(CLIENT)
        environment = {key: value for key, value in os.environ.items()
                       if not key.startswith("PIPEWIRE_")}
        environment.update(XDG_RUNTIME_DIR=temporary, XDG_CONFIG_HOME=temporary,
                           PIPEWIRE_RUNTIME_DIR=temporary, PIPEWIRE_CONFIG_DIR=temporary,
                           PIPEWIRE_REMOTE="just-speak-test-0")
        environment.pop("DBUS_SESSION_BUS_ADDRESS", None)

        def run(*args):
            return subprocess.run(args, env=environment, capture_output=True, text=True,
                                  check=True, timeout=3).stdout

        server = recorder = None
        try:
            with (root / "server.log").open("w") as logs, (root / "record.log").open("w") as record_log:
                server = subprocess.Popen(["/usr/bin/pipewire", "-c", "testserver.conf"],
                                          env=environment, stdout=logs, stderr=logs)
                deadline = time.monotonic() + 3
                while not (root / "just-speak-test-0").exists():
                    assert server.poll() is None, "private PipeWire server exited"
                    assert time.monotonic() < deadline, "private PipeWire socket was not created"
                    time.sleep(0.03)
                wav = root / "record.wav"
                recorder = subprocess.Popen([
                    "/usr/bin/pw-record", "--rate", "16000", "--channels", "1",
                    "--format", "s16", "--container", "wav", "--properties",
                    "{ node.name = JustSpeakTestRecorder node.autoconnect = false }", str(wav),
                ], env=environment, stdout=record_log, stderr=record_log)
                # Manual routing replaces a session manager; only these two
                # synthetic ports exist in the isolated server.
                deadline = time.monotonic() + 3
                while True:
                    outputs = run("/usr/bin/pw-link", "-o").splitlines()
                    inputs = run("/usr/bin/pw-link", "-i").splitlines()
                    source = next((port for port in outputs if port.startswith("JustSpeakTestSource:")), None)
                    sink = next((port for port in inputs if port.startswith("JustSpeakTestRecorder:")), None)
                    if source and sink:
                        break
                    assert time.monotonic() < deadline, "synthetic recording ports did not appear"
                    time.sleep(0.03)
                run("/usr/bin/pw-link", source, sink)
                time.sleep(0.6)
                assert recorder.poll() is None, "recorder exited before requested stop"
                start = time.monotonic()
                recorder.send_signal(signal.SIGINT)
                code = recorder.wait(timeout=2)
                shutdown = time.monotonic() - start
                # PipeWire 1.6.8 returns 1 on a normal recording SIGINT; only
                # drained playback exits 0. The finalized WAV is authoritative.
                assert code in (0, 1, -signal.SIGINT), f"unexpected recorder exit: {code}"
                with wave.open(str(wav)) as audio:
                    params = audio.getparams()
                    raw = audio.readframes(audio.getnframes())
                assert (params.nchannels, params.framerate, params.sampwidth) == (1, 16000, 2), params
                assert params.nframes > 1600, "no substantial audio captured"
                blob = wav.read_bytes()
                assert blob[:4] == b"RIFF" and blob[8:12] == b"WAVE"
                assert blob[12:16] == b"fmt " and blob[20:22] == b"\x01\x00", "expected PCM WAV"
                assert int.from_bytes(blob[4:8], "little") + 8 == len(blob), "WAV header not finalized"
                samples = struct.unpack("<" + "h" * (len(raw) // 2), raw)
                assert max(samples) > 100 and min(samples) < -100, "synthetic source was silent"
                print(f"PASS real pw-record: {params.nframes / 16000:.3f}s PCM16 mono 16kHz; "
                      f"SIGINT exit={code}, shutdown={shutdown * 1000:.1f}ms, peak={max(samples)}")
                print("No physical microphone, speaker, or desktop audio server was accessed.")
        except Exception:
            for name in ["server.log", "record.log"]:
                if (root / name).exists():
                    print(name, (root / name).read_text())
            raise
        finally:
            for process in [recorder, server]:
                if process and process.poll() is None:
                    process.terminate()
                    try:
                        process.wait(timeout=2)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait()


if __name__ == "__main__":
    main()
