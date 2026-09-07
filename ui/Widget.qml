import QtQuick
import Quickshell

// Omarchy injects bar, moduleName and settings into third-party bar widgets.
Item {
    id: root
    property var bar: null
    property string moduleName: "local.just-speak"
    property var settings: ({})
    implicitWidth: 30
    implicitHeight: bar ? bar.barSize : 26

    StatusFeed { id: statusFeed }
    RecordingOverlay {
        feed: statusFeed
        // Omarchy creates one bar surface per monitor. Keep each overlay on
        // its own monitor rather than stacking every instance on the first.
        targetScreen: root.QsWindow.window ? root.QsWindow.window.screen : Quickshell.screens[0] || null
    }

    Text {
        anchors.centerIn: parent
        text: statusFeed.phase === "recording" ? "●" : statusFeed.phase === "transcribing" ? "◌" : "󰍬"
        textFormat: Text.PlainText
        font.family: root.bar ? root.bar.fontFamily : "monospace"
        font.pixelSize: 16
        color: statusFeed.phase === "recording" ? "#ff8585"
               : statusFeed.phase === "error" || statusFeed.phase === "disconnected" ? "#f3c47c"
               : root.bar ? root.bar.foreground : "#f3f4f6"
        opacity: statusFeed.phase === "idle" && !statusFeed.modelReady ? 0.45 : 1
    }

    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        cursorShape: Qt.PointingHandCursor
        acceptedButtons: Qt.LeftButton | Qt.RightButton
        onClicked: mouse => {
            if (mouse.button === Qt.RightButton && statusFeed.canCancel) statusFeed.act("cancel");
            else if (mouse.button === Qt.LeftButton) {
                if (statusFeed.phase === "recording") statusFeed.act("stop");
                else if (statusFeed.modelReady && ["idle", "error"].includes(statusFeed.phase)) statusFeed.act("start");
            }
        }
        onEntered: {
            if (root.bar) root.bar.showTooltip(root, "JustSpeak · " + statusFeed.label
                + (statusFeed.message ? "\n" + statusFeed.message : "")
                + "\nLeft click: start/stop · Right click: cancel");
        }
        onExited: if (root.bar) root.bar.hideTooltip(root)
    }
}
