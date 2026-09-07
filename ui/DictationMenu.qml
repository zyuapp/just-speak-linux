import QtQuick
import QtQuick.Controls as Controls
import qs.Ui as Ui
import qs.Commons

Ui.KeyboardPanel {
    id: root
    required property StatusFeed feed
    required property MenuModel menuModel
    required property UpdateModel updateModel
    property var deferredArguments: []
    readonly property bool editable: menuModel.canEdit && !updateModel.installing
    focusTarget: content
    contentWidth: root.fittedContentWidth(Style.space(410))
    contentHeight: root.fittedContentHeight(column.implicitHeight, Style.space(720))

    function outsideAction(args: list<string>): void {
        deferredArguments = args;
        close();
        outsideDelay.restart();
    }

    onOpenChanged: {
        if (open) shortcutField.text = menuModel.data.settings.shortcut || feed.shortcut;
        else microphone.close();
    }

    property Timer outsideDelayTimer: Timer {
        id: outsideDelay
        // KeyboardPanel releases focus immediately and fades for 140 ms.
        // Let it unmap before the CLI captures the paste target.
        interval: 180
        onTriggered: root.menuModel.run(root.deferredArguments, "")
    }

    property Connections menuConnections: Connections {
        target: root.menuModel
        function onRefreshed(): void {
            if (!shortcutField.activeFocus) shortcutField.text = root.menuModel.data.settings.shortcut || root.feed.shortcut;
        }
    }

    Item {
        id: content
        anchors.fill: parent
        focus: true
        Keys.onEscapePressed: event => {
            if (!microphone.popupOpen) root.close();
            event.accepted = true;
        }

        Controls.ScrollView {
            id: scroll
            anchors.fill: parent
            clip: true
            contentWidth: availableWidth
            Controls.ScrollBar.horizontal.policy: Controls.ScrollBar.AlwaysOff

            Column {
                id: column
                width: scroll.availableWidth
                spacing: Style.space(10)

                Row {
                    width: parent.width
                    spacing: Style.space(10)
                    Column {
                        width: parent.width - refreshButton.implicitWidth - parent.spacing
                        spacing: Style.space(3)
                        Text {
                            text: "JustSpeak"
                            textFormat: Text.PlainText
                            font.family: Style.font.family
                            font.pixelSize: Style.font.subtitle
                            font.bold: true
                            color: Color.popups.text
                        }
                        Text {
                            width: parent.width
                            text: root.feed.label
                            textFormat: Text.PlainText
                            font.family: Style.font.family
                            font.pixelSize: Style.font.caption
                            color: root.feed.phase === "recording" ? "#ff8585" : Color.popups.text
                            wrapMode: Text.Wrap
                        }
                    }
                    Ui.Button {
                        id: refreshButton
                        text: root.menuModel.loading ? "Loading…" : "Refresh"
                        enabled: !root.menuModel.loading && !root.menuModel.busy
                        focusable: true
                        onClicked: { root.menuModel.error = ""; root.menuModel.refresh(); }
                    }
                }

                Text {
                    visible: root.menuModel.error !== "" || root.menuModel.notice !== ""
                    width: parent.width
                    text: root.menuModel.error || root.menuModel.notice
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    maximumLineCount: 5
                    elide: Text.ElideRight
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    color: root.menuModel.error ? "#e7ae83" : Color.popups.text
                }

                Row {
                    width: parent.width
                    spacing: Style.space(8)
                    Ui.Button {
                        text: root.feed.phase === "disconnected" ? "Start JustSpeak"
                              : root.feed.phase === "recording" ? "Finish dictation" : "Start dictation"
                        focusable: true
                        bordered: true
                        enabled: !root.menuModel.busy && !root.updateModel.installing && (root.feed.phase === "disconnected"
                                 || root.feed.phase === "recording" || (root.feed.modelReady && ["idle", "error"].includes(root.feed.phase)))
                        opacity: enabled ? 1 : 0.45
                        onClicked: {
                            if (root.feed.phase === "disconnected") root.menuModel.run(["launch"], "Starting JustSpeak");
                            else root.outsideAction([root.feed.phase === "recording" ? "stop" : "start"]);
                        }
                    }
                    Ui.Button {
                        text: "Cancel"
                        visible: root.feed.canCancel
                        enabled: !root.menuModel.busy
                        focusable: true
                        onClicked: root.menuModel.run(["cancel"], "Canceled")
                    }
                }

                Ui.PanelSeparator { width: parent.width; foreground: Color.popups.text }

                Row {
                    width: parent.width
                    Text {
                        width: parent.width - clearButton.implicitWidth
                        anchors.verticalCenter: parent.verticalCenter
                        text: "Recent transcripts"
                        textFormat: Text.PlainText
                        font.family: Style.font.family
                        font.pixelSize: Style.font.body
                        font.bold: true
                        color: Color.popups.text
                    }
                    Ui.Button {
                        id: clearButton
                        text: "Clear"
                        focusable: true
                        enabled: root.editable && root.menuModel.data.history.length > 0
                        opacity: enabled ? 1 : 0.4
                        onClicked: root.menuModel.run(["history", "clear"], "History cleared")
                    }
                }

                Text {
                    width: parent.width
                    visible: root.menuModel.data.history.length === 0
                    text: root.menuModel.data.settings.history_enabled === false
                          ? "History is off. New transcripts are not saved." : "Your last 10 transcripts will appear here."
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    color: Color.muted
                }

                Controls.ScrollView {
                    id: historyScroll
                    visible: root.menuModel.data.history.length > 0
                    width: parent.width
                    height: Math.min(historyColumn.implicitHeight, Style.space(210))
                    contentWidth: availableWidth
                    clip: true
                    Controls.ScrollBar.horizontal.policy: Controls.ScrollBar.AlwaysOff

                    Column {
                        id: historyColumn
                        width: historyScroll.availableWidth
                        spacing: Style.space(5)
                        Repeater {
                            model: root.menuModel.data.history
                            delegate: Rectangle {
                                id: transcript
                                required property var modelData
                                width: historyColumn.width
                                height: Math.max(Style.space(64), transcriptText.implicitHeight + Style.space(24))
                                radius: Style.cornerRadius
                                color: pasteArea.containsMouse || activeFocus ? Qt.rgba(1, 1, 1, 0.08) : Qt.rgba(1, 1, 1, 0.03)
                                activeFocusOnTab: root.editable
                                Keys.onReturnPressed: if (root.editable) root.outsideAction(["history", "paste", String(modelData.id)])
                                Keys.onSpacePressed: if (root.editable) root.outsideAction(["history", "paste", String(modelData.id)])

                                MouseArea {
                                    id: pasteArea
                                    anchors.fill: parent
                                    anchors.rightMargin: copyButton.width + Style.space(8)
                                    hoverEnabled: true
                                    enabled: root.editable
                                    cursorShape: Qt.PointingHandCursor
                                    onClicked: root.outsideAction(["history", "paste", String(transcript.modelData.id)])
                                }
                                Column {
                                    anchors.left: parent.left
                                    anchors.right: copyButton.left
                                    anchors.verticalCenter: parent.verticalCenter
                                    anchors.margins: Style.space(8)
                                    spacing: Style.space(3)
                                    Text {
                                        id: transcriptText
                                        width: parent.width
                                        text: String(transcript.modelData.text)
                                        textFormat: Text.PlainText
                                        font.family: Style.font.family
                                        font.pixelSize: Style.font.body
                                        color: Color.popups.text
                                        maximumLineCount: 2
                                        wrapMode: Text.Wrap
                                        elide: Text.ElideRight
                                    }
                                    Text {
                                        text: Qt.formatDateTime(new Date(Number(transcript.modelData.created_at) * 1000), "ddd HH:mm") + " · click to paste"
                                        textFormat: Text.PlainText
                                        font.family: Style.font.family
                                        font.pixelSize: Style.font.caption
                                        color: Color.muted
                                    }
                                }
                                Ui.Button {
                                    id: copyButton
                                    anchors.right: parent.right
                                    anchors.verticalCenter: parent.verticalCenter
                                    anchors.rightMargin: Style.space(4)
                                    text: "Copy"
                                    focusable: true
                                    enabled: root.editable
                                    onClicked: root.menuModel.run(["history", "copy", String(transcript.modelData.id)], "Copied to clipboard")
                                }
                            }
                        }
                    }
                }

                Ui.PanelSeparator { width: parent.width; foreground: Color.popups.text }

                Ui.Dropdown {
                    id: microphone
                    width: parent.width
                    label: "Microphone"
                    value: root.menuModel.data.settings.input === null || root.menuModel.data.settings.input === undefined
                           ? "default" : String(root.menuModel.data.settings.input)
                    options: root.menuModel.inputs
                    enabled: root.editable
                    opacity: enabled ? 1 : 0.45
                    onChanged: value => root.menuModel.run(["input", "set", value], "Microphone updated")
                }
                Text {
                    width: parent.width
                    visible: !!root.menuModel.data.input_error
                    text: root.menuModel.data.input_error || ""
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    color: "#e7ae83"
                }

                Column {
                    width: parent.width
                    spacing: Style.space(5)
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
                        Ui.TextField {
                            id: shortcutField
                            width: parent.width - applyShortcut.implicitWidth - parent.spacing
                            enabled: root.editable
                            placeholderText: "e.g. SUPER + F10"
                            maximumLength: 96
                            selectByMouse: true
                            onAccepted: if (root.editable && text.trim()) root.menuModel.run(["shortcut", "set", text.trim()], "Shortcut updated")
                        }
                        Ui.Button {
                            id: applyShortcut
                            text: "Apply"
                            focusable: true
                            enabled: root.editable && shortcutField.text.trim() !== "" && shortcutField.text.trim() !== root.feed.shortcut
                            opacity: enabled ? 1 : 0.45
                            onClicked: root.menuModel.run(["shortcut", "set", shortcutField.text.trim()], "Shortcut updated")
                        }
                    }
                }

                Repeater {
                    model: [
                        { key: "sound_feedback", label: "Recording sounds" },
                        { key: "mute_while_recording", label: "Mute other audio while recording" },
                        { key: "paste", label: "Paste automatically" },
                        { key: "history_enabled", label: "Save recent transcripts" },
                        { key: "auto_check_updates", label: "Check for updates automatically" }
                    ]
                    delegate: Item {
                        id: settingRow
                        required property var modelData
                        width: column.width
                        height: Style.space(34)
                        enabled: root.editable
                        opacity: enabled ? 1 : 0.45
                        activeFocusOnTab: enabled
                        Keys.onReturnPressed: root.menuModel.toggle(modelData.key)
                        Keys.onSpacePressed: root.menuModel.toggle(modelData.key)
                        Text {
                            anchors.left: parent.left
                            anchors.right: toggleSwitch.left
                            anchors.verticalCenter: parent.verticalCenter
                            text: String(settingRow.modelData.label)
                            textFormat: Text.PlainText
                            elide: Text.ElideRight
                            font.family: Style.font.family
                            font.pixelSize: Style.font.body
                            color: Color.popups.text
                        }
                        Ui.ToggleSwitch {
                            id: toggleSwitch
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            checked: root.menuModel.data.settings[settingRow.modelData.key] === true
                            interactive: false
                            hasCursor: settingArea.containsMouse || settingRow.activeFocus
                        }
                        MouseArea {
                            id: settingArea
                            anchors.fill: parent
                            hoverEnabled: true
                            cursorShape: Qt.PointingHandCursor
                            onClicked: root.menuModel.toggle(settingRow.modelData.key)
                        }
                    }
                }

                Ui.PanelSeparator { width: parent.width; foreground: Color.popups.text }

                Text {
                    width: parent.width
                    text: root.updateModel.label
                    textFormat: Text.PlainText
                    wrapMode: Text.Wrap
                    font.family: Style.font.family
                    font.pixelSize: Style.font.caption
                    color: root.updateModel.error ? "#e7ae83" : Color.popups.text
                }
                Row {
                    spacing: Style.space(6)
                    Ui.Button {
                        text: root.updateModel.checking ? "Checking…" : "Check for updates"
                        enabled: !root.updateModel.checking && !root.updateModel.installing
                        focusable: true
                        onClicked: root.updateModel.check(true)
                    }
                    Ui.Button {
                        text: root.updateModel.installing ? "Installing…" : "Install update"
                        visible: root.updateModel.available || root.updateModel.installing
                        enabled: !root.menuModel.busy && !root.menuModel.recording && !root.updateModel.installing && !root.updateModel.checking
                        focusable: true
                        onClicked: root.updateModel.install()
                    }
                }

                Row {
                    width: parent.width
                    spacing: Style.space(6)
                    Text {
                        width: parent.width - restartButton.implicitWidth - quitButton.implicitWidth - parent.spacing * 2
                        anchors.verticalCenter: parent.verticalCenter
                        text: "v" + (root.menuModel.data.version || "0.2.0")
                        textFormat: Text.PlainText
                        font.family: Style.font.family
                        font.pixelSize: Style.font.caption
                        color: Color.muted
                    }
                    Ui.Button {
                        id: restartButton
                        text: "Restart"
                        enabled: !root.menuModel.busy && !root.menuModel.recording && !root.updateModel.installing
                        opacity: enabled ? 1 : 0.4
                        focusable: true
                        onClicked: root.menuModel.run(["restart"], "Restarting JustSpeak")
                    }
                    Ui.Button {
                        id: quitButton
                        text: "Quit"
                        enabled: !root.menuModel.busy && !root.updateModel.installing && root.feed.phase !== "disconnected"
                        opacity: enabled ? 1 : 0.4
                        focusable: true
                        onClicked: { root.menuModel.run(["quit"], "JustSpeak stopped"); root.close(); }
                    }
                }
            }
        }
    }
}
