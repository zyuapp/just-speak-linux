import QtQuick
import Quickshell
import Quickshell.Io

Scope {
    id: root
    required property StatusFeed feed
    required property MenuModel menuModel
    property bool checking: false
    property bool installing: false
    property bool available: false
    property bool checked: false
    property string latestVersion: ""
    property string releaseUrl: ""
    property string error: ""
    property string diagnostics: ""
    property string notice: ""
    property bool checkTimedOut: false
    property real lastCheck: 0
    readonly property string label: {
        if (installing) return "Installing update… JustSpeak and the Omarchy bar will reload.";
        if (checking) return "Checking for updates…";
        if (error) return error;
        if (notice) return notice;
        if (available) return "JustSpeak " + latestVersion + " is available.";
        if (checked && !latestVersion) return "No published release yet.";
        if (checked) return "JustSpeak is up to date.";
        return "Updates";
    }

    function check(manual: bool): void {
        if (!feed.executableReady || checking || installing) return;
        if (!manual && Date.now() - lastCheck < 6 * 60 * 60 * 1000) return;
        checking = true;
        checkTimedOut = false;
        lastCheck = Date.now();
        diagnostics = "";
        error = "";
        notice = "";
        checker.startCommand([feed.executable, "update", "check", "--json"]);
        checkTimeout.restart();
    }

    function install(): void {
        if (!available || checking || installing || menuModel.busy || menuModel.recording) return;
        installing = true;
        diagnostics = "";
        error = "";
        notice = "";
        installer.startCommand([feed.executable, "update", "install"]);
    }

    CommandProcess {
        id: checker
        onFailedToStart: root.diagnostics = "Could not start JustSpeak. Reopen the app and try again."
        stdout: StdioCollector { id: response }
        stderr: SplitParser {
            onRead: line => root.diagnostics = (root.diagnostics + line + "\n").slice(0, 1200)
        }
        onFinished: exitCode => {
            checkTimeout.stop();
            root.checking = false;
            if (root.checkTimedOut) return;
            if (exitCode !== 0) {
                root.error = root.diagnostics.trim() || "Could not check for updates. Try again when you are online.";
                return;
            }
            try {
                const result = JSON.parse(response.text);
                if (typeof result.available !== "boolean" || typeof result.latest_version !== "string"
                    || (result.available && !result.latest_version)
                    || (result.release_url !== undefined && typeof result.release_url !== "string"))
                    throw new Error("Invalid update response");
                root.available = result.available;
                root.latestVersion = result.latest_version || "";
                root.releaseUrl = result.release_url || "";
                root.checked = true;
            } catch (failure) {
                root.error = "Could not read the update information. Try again later.";
            }
        }
    }

    CommandProcess {
        id: installer
        onFailedToStart: root.diagnostics = "Could not start JustSpeak. Reopen the app and try again."
        stdout: SplitParser { onRead: line => {} }
        stderr: SplitParser {
            onRead: line => root.diagnostics = (root.diagnostics + line + "\n").slice(0, 1200)
        }
        onFinished: exitCode => {
            root.installing = false;
            if (exitCode === 0) {
                root.available = false;
                root.notice = "Update installed. JustSpeak is restarting.";
                root.menuModel.refresh();
            } else {
                root.error = root.diagnostics.trim() || "The update needs attention. Check the installed version and update log.";
            }
        }
    }

    Timer {
        id: checkTimeout
        interval: 45000
        onTriggered: {
            root.checkTimedOut = true;
            checker.running = false;
            root.error = "Checking for updates timed out. Try again later.";
        }
    }
    Timer {
        interval: 30000
        repeat: true
        running: true
        onTriggered: {
            if (root.menuModel.ready && root.menuModel.data.settings.auto_check_updates !== false
                && root.feed.phase === "idle") root.check(false);
        }
    }
}
