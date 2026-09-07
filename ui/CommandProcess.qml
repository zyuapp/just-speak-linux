import QtQuick
import Quickshell.Io

// Quickshell emits exited for a running child, but not when exec fails. Every
// submitted UI command needs a completion in both cases to release its controls.
Process {
    id: root
    property bool pending: false
    property bool launched: false
    signal finished(int exitCode)
    signal failedToStart()

    function startCommand(args: list<string>): void {
        if (pending) return;
        launched = false;
        pending = true;
        command = args;
        running = true;
    }

    onStarted: launched = true
    onExited: exitCode => {
        if (!pending) return;
        pending = false;
        finished(exitCode);
    }
    property Timer startDeadline: Timer {
        interval: 1000
        repeat: true
        running: root.pending && !root.launched
        onTriggered: {
            if (root.running) return;
            root.pending = false;
            root.failedToStart();
            root.finished(-1);
        }
    }
}
