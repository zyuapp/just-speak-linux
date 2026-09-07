import QtQuick
import Quickshell
import Quickshell.Wayland

Scope {
    id: root
    required property StatusFeed feed
    property var targetScreen: Quickshell.screens[0] || null
    property bool errorVisible: false

    Connections {
        target: root.feed
        function onPhaseChanged(): void {
            root.errorVisible = root.feed.phase === "error";
            if (root.errorVisible) errorTimeout.restart();
        }
        function onMessageChanged(): void {
            if (root.feed.phase === "error") {
                root.errorVisible = true;
                errorTimeout.restart();
            }
        }
    }

    Timer {
        id: errorTimeout
        interval: 8000
        onTriggered: root.errorVisible = false
    }

    PanelWindow {
        screen: root.targetScreen
        visible: root.feed.phase === "recording" || root.feed.phase === "transcribing"
                 || root.feed.phase === "canceling" || root.feed.phase === "loading" || root.errorVisible
        anchors { bottom: true }
        margins { bottom: 70 }
        implicitWidth: 360
        implicitHeight: root.feed.phase === "error" ? 98 : 72
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "just-speak"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        // An empty input region lets pointer events reach the application below.
        mask: Region {}

        Rectangle {
            anchors.fill: parent
            radius: 14
            color: "#ee20242b"
            border.width: 1
            border.color: root.feed.phase === "recording" ? "#e57373" : "#586273"

            Row {
                anchors.centerIn: parent
                spacing: 14

                Rectangle {
                    anchors.verticalCenter: parent.verticalCenter
                    width: 10
                    height: 10
                    radius: 5
                    color: root.feed.phase === "recording" ? "#ff8585"
                           : root.feed.phase === "error" ? "#f3c47c" : "#9ab7f0"
                    SequentialAnimation on opacity {
                        running: root.feed.phase === "recording"
                        loops: Animation.Infinite
                        NumberAnimation { to: 0.35; duration: 650 }
                        NumberAnimation { to: 1; duration: 650 }
                    }
                }

                Column {
                    width: 290
                    spacing: 5
                    Text {
                        width: parent.width
                        text: root.feed.label
                        textFormat: Text.PlainText
                        elide: Text.ElideRight
                        font.pixelSize: 15
                        font.bold: true
                        color: "#f3f4f6"
                    }
                    Text {
                        width: parent.width
                        text: root.feed.phase === "error" ? root.feed.message
                              : root.feed.phase === "recording" ? "Release " + root.feed.shortcut + " to finish · Esc to cancel"
                              : root.feed.phase === "transcribing" ? "Esc to cancel"
                              : root.feed.phase === "canceling" ? "Releasing the microphone and restoring audio" : "Preparing local dictation"
                        textFormat: Text.PlainText
                        wrapMode: Text.Wrap
                        maximumLineCount: root.feed.phase === "error" ? 3 : 1
                        elide: Text.ElideRight
                        font.pixelSize: 12
                        color: "#b7bfcc"
                    }
                }
            }
        }
    }
}
