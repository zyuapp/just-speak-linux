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
    property string actionName: ""
    property bool actionTimedOut: false
    property bool readTimedOut: false
    property int revision: 0
    property int readRevision: 0
    readonly property bool recording: ["recording", "transcribing", "canceling", "updating", "loading", "stopping"].includes(feed.phase)
    readonly property bool canQuit: !busy && !["updating", "stopping", "disconnected"].includes(feed.phase)
    readonly property bool canEdit: ready && !loading && !busy && !recording && feed.phase !== "disconnected"
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
    signal quitFinished()

    function refresh(): void {
        if (feed.phase === "disconnected" || feed.phase === "stopping") {
            refreshPending = false;
            ready = false;
            return;
        }
        if (!feed.executableReady || loading || busy) {
            refreshPending = true;
            return;
        }
        refreshPending = false;
        loading = true;
        readTimedOut = false;
        readRevision = revision;
        readErrors = "";
        reader.startCommand([feed.executable, "menu", "--json"]);
        readTimeout.restart();
    }

    function run(args: list<string>, success: string): bool {
        if (busy || !feed.executableReady) return false;
        if (recording && !["cancel", "quit", "stop"].includes(args[0])) {
            error = "Finish or cancel dictation before changing settings.";
            return false;
        }
        busy = true;
        revision++;
        error = "";
        notice = "";
        actionErrors = "";
        actionName = args[0];
        actionTimedOut = false;
        if (actionName === "quit") notice = "Stopping JustSpeak…";
        successMessage = success;
        let command = [feed.executable];
        for (const arg of args) command.push(arg);
        action.startCommand(command);
        actionTimeout.interval = args[0] === "quit" ? 35000 : args[0] === "shortcut" ? 25000 : 10000;
        actionTimeout.restart();
        return true;
    }

    function toggle(key: string): void {
        if (canEdit) run(["settings", "set", key, data.settings[key] === true ? "false" : "true"], "Settings saved");
    }

    Connections {
        target: root.feed
        function onExecutableReadyChanged(): void {
            if (root.feed.executableReady && root.refreshPending) root.refresh();
        }
        function onPhaseChanged(): void {
            if (["disconnected", "stopping"].includes(root.feed.phase)) {
                root.ready = false;
                root.revision++;
            }
            else if (!root.busy && ["loading", "idle", "error"].includes(root.feed.phase)) root.refresh();
        }
    }

    CommandProcess {
        id: reader
        onFailedToStart: root.readErrors = "Could not start JustSpeak. Reopen the app and try again."
        stdout: StdioCollector { id: menuOutput }
        stderr: SplitParser {
            onRead: line => root.readErrors = (root.readErrors + line + "\n").slice(0, 1200)
        }
        onFinished: exitCode => {
            readTimeout.stop();
            root.loading = false;
            if (["disconnected", "stopping"].includes(root.feed.phase)) return;
            if (root.busy || root.readRevision !== root.revision) {
                root.ready = false;
                root.refreshPending = true;
                if (!root.busy) Qt.callLater(root.refresh);
                return;
            }
            if (root.readTimedOut) {
                root.ready = false;
            } else if (exitCode === 0) {
                try {
                    const result = JSON.parse(menuOutput.text);
                    if (!result.settings || typeof result.settings !== "object" || Array.isArray(result.settings)
                        || !Array.isArray(result.inputs) || !Array.isArray(result.history))
                        throw new Error("Invalid menu response");
                    result.history = result.history.slice(0, 10);
                    root.data = result;
                    root.ready = true;
                    if (typeof result.settings.shortcut === "string") root.feed.shortcut = result.settings.shortcut;
                    root.refreshed();
                } catch (failure) {
                    root.ready = false;
                    root.error = root.error || "Could not read JustSpeak settings. Restart JustSpeak and try again.";
                }
            } else {
                root.ready = false;
                root.error = root.error || root.readErrors.trim() || "Could not load JustSpeak settings.";
            }
            if (root.refreshPending) Qt.callLater(root.refresh);
        }
    }

    CommandProcess {
        id: action
        onFailedToStart: root.actionErrors = "Could not start JustSpeak. Reopen the app and try again."
        stdout: SplitParser { onRead: line => {} }
        stderr: SplitParser {
            onRead: line => root.actionErrors = (root.actionErrors + line + "\n").slice(0, 1200)
        }
        onFinished: exitCode => {
            actionTimeout.stop();
            root.busy = false;
            const ok = exitCode === 0 && !root.actionTimedOut;
            if (ok) {
                root.notice = root.successMessage;
                noticeTimeout.restart();
                if (root.actionName === "quit") {
                    root.feed.disconnected();
                    root.quitFinished();
                } else root.refresh();
            } else {
                root.notice = "";
                if (!root.actionTimedOut)
                    root.error = root.actionErrors.trim() || "JustSpeak could not complete that action.";
                if (root.refreshPending) root.refresh();
            }
            root.actionFinished(ok);
        }
    }

    Timer {
        id: readTimeout
        interval: 8000
        onTriggered: {
            root.readTimedOut = true;
            reader.running = false;
            if (!root.busy && root.readRevision === root.revision)
                root.error = root.error || "Loading settings timed out. Try Refresh.";
        }
    }
    Timer {
        id: actionTimeout
        interval: 10000
        onTriggered: {
            root.actionTimedOut = true;
            action.running = false;
            root.error = "The action timed out. Refresh to check its result.";
        }
    }
    Timer {
        id: noticeTimeout
        interval: 3500
        onTriggered: root.notice = ""
    }
}
