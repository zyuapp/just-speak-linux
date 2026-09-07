import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root

    readonly property string overrideExecutable: Quickshell.env("JUST_SPEAK_BIN") || ""
    readonly property string localExecutable: Quickshell.env("HOME") + "/.local/bin/just-speak"
    property string executable: overrideExecutable || "just-speak"
    property bool executableReady: overrideExecutable !== ""
    property string phase: "disconnected"
    property string message: "Start the JustSpeak service to connect."
    property real elapsedSeconds: 0
    property bool modelReady: false
    property string modelSetup: ""
    property bool canCancel: false
    property string shortcut: "F10"
    readonly property string label: {
        if (phase === "recording") return "Listening · " + Math.floor(elapsedSeconds) + "s";
        if (phase === "transcribing") return "Transcribing…";
        if (phase === "canceling") return "Canceling dictation…";
        if (phase === "loading") return message || "Loading speech model…";
        if (phase === "updating") return message || "Updating JustSpeak…";
        if (phase === "stopping") return "Stopping JustSpeak…";
        if (phase === "error" && modelSetup === "required") return "Set up offline dictation";
        if (phase === "error" && modelSetup === "failed") return "Model download needs attention";
        if (phase === "error") return "JustSpeak needs attention";
        if (phase === "disconnected") return "JustSpeak is offline";
        return modelReady ? "Ready · hold " + shortcut + " to speak" : "Speech model not ready";
    }

    function update(line: string): void {
        try {
            const state = JSON.parse(line);
            if (!["loading", "idle", "recording", "transcribing", "canceling", "error", "updating", "stopping"].includes(state.phase))
                return;
            message = typeof state.message === "string" ? state.message : "";
            elapsedSeconds = typeof state.elapsed_seconds === "number" ? Math.max(0, state.elapsed_seconds) : 0;
            modelReady = state.model_ready === true;
            modelSetup = typeof state.model_setup === "string" ? state.model_setup : "";
            canCancel = typeof state.can_cancel === "boolean" ? state.can_cancel
                : state.phase === "recording" || state.phase === "transcribing";
            if (typeof state.shortcut === "string" && state.shortcut.length > 0) shortcut = state.shortcut;
            phase = state.phase;
        } catch (error) {
            console.warn("JustSpeak: ignored malformed status event");
        }
    }

    function act(action: string): void {
        Quickshell.execDetached([executable, action]);
    }

    function disconnected(): void {
        phase = "disconnected";
        message = "Open JustSpeak to start dictating.";
        modelReady = false;
        modelSetup = "";
        canCancel = false;
    }

    Process {
        command: ["/usr/bin/test", "-x", root.localExecutable]
        running: !root.executableReady
        onExited: exitCode => {
            root.executable = exitCode === 0 ? root.localExecutable : "just-speak";
            root.executableReady = true;
        }
    }

    Process {
        id: watcher
        command: [root.executable, "watch"]
        running: root.executableReady
        stdout: SplitParser {
            onRead: data => root.update(data)
        }
        // Diagnostics may contain paths or transcript context: keep them out of
        // the desktop shell log. The daemon's status message is the UI contract.
        stderr: StdioCollector {}
        onRunningChanged: {
            if (!running) {
                root.disconnected();
            }
        }
    }

    Timer {
        interval: 2000
        repeat: true
        running: root.executableReady && !watcher.running
        onTriggered: watcher.running = true
    }
}
