import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root
    required property StatusFeed feed
    property var data: ({ settings: {}, inputs: [], history: [], version: "" })
    property bool ready: false
    property bool loading: false
    property bool busy: false
    property bool refreshPending: false
    property string error: ""
    property string notice: ""
    property string actionErrors: ""
    property string readErrors: ""
    property string successMessage: ""
    readonly property bool recording: ["recording", "transcribing", "updating"].includes(feed.phase)
    readonly property bool canEdit: ready && !busy && !recording && feed.phase !== "disconnected"
    readonly property var inputs: {
        let options = [{ value: "default", label: "System default microphone" }];
        for (const input of data.inputs || []) {
            options.push({ value: String(input.id), label: String(input.name) + (input.is_default ? " · default" : "") });
        }
        const selected = data.settings.input;
        if (selected !== null && selected !== undefined && !options.some(option => option.value === String(selected)))
            options.push({ value: String(selected), label: "Unavailable input · " + selected });
        return options;
    }
    signal refreshed()
    signal actionFinished(bool ok)

    function refresh(): void {
        if (!feed.executableReady || loading) {
            refreshPending = true;
            return;
        }
        refreshPending = false;
        loading = true;
        readErrors = "";
        reader.command = [feed.executable, "menu", "--json"];
        reader.running = true;
        readTimeout.restart();
    }

    function run(args: list<string>, success: string): void {
        if (busy || !feed.executableReady) return;
        if (recording && !["cancel", "quit", "stop"].includes(args[0])) {
            error = "Finish or cancel dictation before changing settings.";
            return;
        }
        busy = true;
        error = "";
        notice = "";
        actionErrors = "";
        successMessage = success;
        let command = [feed.executable];
        for (const arg of args) command.push(arg);
        action.command = command;
        action.running = true;
        actionTimeout.interval = args[0] === "shortcut" ? 25000 : 10000;
        actionTimeout.restart();
    }

    function toggle(key: string): void {
        if (canEdit) run(["settings", "set", key, data.settings[key] === true ? "false" : "true"], "Settings saved");
    }

    Connections {
        target: root.feed
        function onExecutableReadyChanged(): void {
            if (root.feed.executableReady && root.refreshPending) root.refresh();
        }
    }

    Process {
        id: reader
        stdout: StdioCollector { id: menuOutput }
        stderr: SplitParser {
            onRead: line => root.readErrors = (root.readErrors + line + "\n").slice(0, 1200)
        }
        onExited: exitCode => {
            readTimeout.stop();
            root.loading = false;
            if (exitCode === 0) {
                try {
                    const result = JSON.parse(menuOutput.text);
                    if (!result.settings || !Array.isArray(result.inputs) || !Array.isArray(result.history))
                        throw new Error("Invalid menu response");
                    result.history = result.history.slice(0, 10);
                    root.data = result;
                    root.ready = true;
                    if (typeof result.settings.shortcut === "string") root.feed.shortcut = result.settings.shortcut;
                    root.refreshed();
                } catch (failure) {
                    root.error = "Could not read JustSpeak settings. Restart JustSpeak and try again.";
                }
            } else {
                root.error = root.readErrors.trim() || "Could not load JustSpeak settings.";
            }
            if (root.refreshPending) Qt.callLater(root.refresh);
        }
    }

    Process {
        id: action
        stdout: SplitParser { onRead: line => {} }
        stderr: SplitParser {
            onRead: line => root.actionErrors = (root.actionErrors + line + "\n").slice(0, 1200)
        }
        onExited: exitCode => {
            actionTimeout.stop();
            root.busy = false;
            if (exitCode === 0) {
                root.notice = root.successMessage;
                noticeTimeout.restart();
                root.refresh();
            } else {
                root.error = root.actionErrors.trim() || "JustSpeak could not complete that action.";
            }
            root.actionFinished(exitCode === 0);
        }
    }

    Timer {
        id: readTimeout
        interval: 8000
        onTriggered: {
            reader.running = false;
            root.loading = false;
            root.error = "Loading settings timed out. Try Refresh.";
        }
    }
    Timer {
        id: actionTimeout
        interval: 10000
        onTriggered: {
            action.running = false;
            root.busy = false;
            root.error = "The action timed out. Refresh to check its result.";
        }
    }
    Timer {
        id: noticeTimeout
        interval: 3500
        onTriggered: root.notice = ""
    }
}
