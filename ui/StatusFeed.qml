import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root

    readonly property string executable: Quickshell.env("JUST_SPEAK_BIN") || "just-speak"
    property string phase: "disconnected"
    property string message: "Start the JustSpeak service to connect."
    property real elapsedSeconds: 0
    property bool modelReady: false
    property bool canCancel: false
    readonly property string label: {
        if (phase === "recording") return "Listening · " + Math.floor(elapsedSeconds) + "s";
        if (phase === "transcribing") return "Transcribing…";
        if (phase === "loading") return "Loading speech model…";
        if (phase === "error") return "JustSpeak needs attention";
        if (phase === "disconnected") return "JustSpeak is offline";
        return modelReady ? "Ready · hold F10 to speak" : "Speech model not ready";
    }

    function update(line: string): void {
        try {
            const state = JSON.parse(line);
            if (!["loading", "idle", "recording", "transcribing", "error"].includes(state.phase))
                return;
            message = typeof state.message === "string" ? state.message : "";
            elapsedSeconds = typeof state.elapsed_seconds === "number" ? Math.max(0, state.elapsed_seconds) : 0;
            modelReady = state.model_ready === true;
            canCancel = state.can_cancel === true || state.phase === "recording" || state.phase === "transcribing";
            phase = state.phase;
        } catch (error) {
            console.warn("JustSpeak: ignored malformed status event");
        }
    }

    function act(action: string): void {
        Quickshell.execDetached([executable, action]);
    }

    Process {
        id: watcher
        command: [root.executable, "watch"]
        running: true
        stdout: SplitParser {
            onRead: data => root.update(data)
        }
        // Diagnostics may contain paths or transcript context: keep them out of
        // the desktop shell log. The daemon's status message is the UI contract.
        stderr: StdioCollector {}
        onRunningChanged: {
            if (!running) {
                root.phase = "disconnected";
                root.message = "Start the JustSpeak service to connect.";
                root.modelReady = false;
                root.canCancel = false;
            }
        }
    }

    Timer {
        interval: 2000
        repeat: true
        running: !watcher.running
        onTriggered: watcher.running = true
    }
}
