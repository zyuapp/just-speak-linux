import QtQuick
import Quickshell.Io
import Quickshell.Wayland
import qs.Ui as Ui
import qs.Commons

Column {
    id: root
    objectName: "shortcutRecorder"
    required property var panel
    required property MenuModel menuModel
    required property bool available
    property alias capture: capture
    readonly property bool editing: capture.editing
    property bool closePanelPending: false
    property var resolution: null
    property bool protectionGranted: false
    signal closePanelRequested()
    spacing: Style.space(5)
    focus: editing
    Keys.onPressed: event => capture.press(event)
    Keys.onReleased: event => capture.release(event)

    function begin() {
        if (!available) return;
        menuModel.error = ""; menuModel.notice = "";
        closePanelPending = false;
        protectionGranted = false;
        capture.begin();
        forceActiveFocus();
    }
    onAvailableChanged: {
        if (!available && editing && !capture.saving) capture.cancel();
    }
    function requestPanelClose() {
        if (!editing) return true;
        closePanelPending = true;
        capture.cancel();
        return !editing;
    }

    ShortcutInhibitor {
        id: inhibitor
        window: root.panel
        enabled: root.editing && root.panel.open
        onActiveChanged: if (active) root.protectionGranted = true
    }
    ShortcutCapture {
        id: capture
        protectedInput: inhibitor.active && root.panel.open
        onResolveRequested: (code, key, generation) => {
            root.resolution = {code: code, key: key, generation: generation};
        }
        onSaveRequested: shortcut => {
            if (!root.menuModel.canEdit) { saved(false, "Finish the current action before saving."); return; }
            root.menuModel.run(["shortcut", "set", shortcut], "Shortcut updated");
        }
        onGenerationChanged: root.resolution = null
        onFinished: {
            if (root.closePanelPending) {
                root.closePanelPending = false;
                root.closePanelRequested();
            } else recordButton.forceActiveFocus();
        }
    }
    Connections {
        target: root.menuModel
        function onActionFinished(ok) { capture.saved(ok, root.menuModel.error); }
    }
    Connections {
        target: root.panel
        function onOpenChanged() {
            // Owner destruction or a bar popout switch can bypass the local
            // close request. Unmapping always releases this surface's inhibitor.
            if (!root.panel.open && capture.editing && !capture.saving) capture.finish();
        }
    }
    // Each lookup owns its process and deadline. Leaving this capture phase
    // destroys both, so canceled or timed-out replies cannot affect a new chord.
    Loader {
        visible: false
        active: root.resolution !== null && root.editing && capture.phase === "resolving"
        sourceComponent: Component {
            Item {
                id: lookup
                property int generation: -1
                Component.onCompleted: {
                    const request = root.resolution;
                    generation = request.generation;
                    process.command = ["gjs", "-m", decodeURIComponent(Qt.resolvedUrl("../gtk/shortcut-keymap.js").toString().replace(/^file:\/\//, "")), String(request.code), String(request.key)];
                    process.running = true;
                }
                Process {
                    id: process
                    stdout: StdioCollector { id: keyOutput }
                    stderr: StdioCollector {}
                    onExited: exitCode => {
                        let result = {};
                        try { result = JSON.parse(keyOutput.text); } catch (_) {}
                        capture.resolved(lookup.generation, result.key || "",
                            result.error || (exitCode === 0 ? "" : "Could not read the keyboard layout. Try recording again."));
                    }
                }
                Timer {
                    interval: 4000
                    running: true
                    onTriggered: capture.resolved(lookup.generation, "", "Reading the keyboard layout timed out. Try again.")
                }
            }
        }
    }
    Timer {
        interval: 4000
        running: root.editing && !root.protectionGranted && !capture.protectedInput && !capture.saving
        onTriggered: {
            // Keep a completed preview while focus is elsewhere. Only the
            // initial grant has a deadline, so denied inhibition fails closed.
            if (capture.phase === "waiting" && !capture.candidate) {
                root.menuModel.error = "Shortcut recording needs keyboard focus and desktop shortcut protection. Click Record shortcut to try again.";
                capture.cancel();
            }
        }
    }
    Timer {
        interval: 30000
        running: root.editing && capture.protectedInput && capture.phase === "recording" && !capture.pendingClose && !capture.saving
        onTriggered: {
            root.menuModel.error = "Shortcut recording timed out. Your previous shortcut is unchanged.";
            capture.cancel();
        }
    }

    Text {
        text: "Hold-to-talk shortcut"
        textFormat: Text.PlainText
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
        color: Color.popups.text
    }
    Row {
        width: parent.width
        spacing: Style.space(6)
        Text {
            width: parent.width - (recordButton.visible ? recordButton.implicitWidth + parent.spacing : 0)
            anchors.verticalCenter: parent.verticalCenter
            text: root.editing ? (capture.candidate || "Press your shortcut")
                : root.menuModel.data.settings.shortcut || root.menuModel.feed.shortcut
            textFormat: Text.PlainText
            font.family: Style.font.family
            font.pixelSize: Style.font.body
            color: Color.popups.text
            elide: Text.ElideRight
        }
        Ui.Button {
            id: recordButton
            text: "Record shortcut…"
            visible: !root.editing
            enabled: root.available
            opacity: enabled ? 1 : 0.45
            focusable: true
            onClicked: root.begin()
        }
    }
    Text {
        width: parent.width
        visible: root.editing
        text: capture.pendingClose ? "Release all keys to finish canceling."
            : capture.saving ? "Saving shortcut…"
            : !capture.protectedInput ? "Recording paused. Click here to resume when desktop shortcut protection is ready."
            : capture.phase === "resolving" ? "Identifying key…"
            : capture.keysDown ? "Release all keys to continue."
            : capture.phase === "preview" ? "Choose Save, Record again, or Cancel. Tab selects a button; Enter activates it."
            : "Press a function key or a combination such as Super + F10. Escape cancels."
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
        color: Color.popups.text
        MouseArea { anchors.fill: parent; onClicked: root.forceActiveFocus() }
    }
    Text {
        width: parent.width
        visible: root.editing && capture.error !== ""
        text: capture.error
        textFormat: Text.PlainText
        wrapMode: Text.Wrap
        font.family: Style.font.family
        font.pixelSize: Style.font.caption
        color: "#e7ae83"
    }
    Row {
        visible: root.editing
        spacing: Style.space(4)
        Ui.Button {
            text: "Save"
            enabled: capture.canSave
            opacity: enabled ? 1 : 0.45
            hasCursor: capture.selection === 0
            onClicked: { root.forceActiveFocus(); capture.commit(); }
        }
        Ui.Button {
            text: "Record again"
            enabled: capture.phase === "preview" && capture.protectedInput && !capture.keysDown && !capture.saving && !capture.pendingClose
            opacity: enabled ? 1 : 0.45
            hasCursor: capture.selection === 1
            onClicked: { root.forceActiveFocus(); capture.retry(); }
        }
        Ui.Button {
            text: "Cancel"
            enabled: !capture.saving && !capture.pendingClose
            opacity: enabled ? 1 : 0.45
            hasCursor: capture.selection === 2
            onClicked: { root.forceActiveFocus(); capture.cancel(); }
        }
    }
}
