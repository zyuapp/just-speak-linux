import QtQuick
import Quickshell
import qs.Ui as Ui

Ui.BarWidget {
    id: root
    moduleName: "local.just-speak"
    property bool opened: false
    property bool popoutSwitchClosing: false
    visible: statusFeed.phase !== "disconnected"
    implicitWidth: visible ? button.implicitWidth : 0
    implicitHeight: visible ? button.implicitHeight : 0

    function open(): void {
        popoutSwitchClosing = false;
        opened = true;
        menuState.refresh();
    }
    function close(): void { if (dictationMenu.requestClose()) opened = false; }
    function toggle(): void { opened ? close() : open(); }
    function closeForPopoutSwitch(): void { popoutSwitchClosing = true; close(); }
    function refresh(): void { menuState.refresh(); }

    StatusFeed { id: statusFeed }
    MenuModel { id: menuState; feed: statusFeed }
    UpdateModel { id: updateState; feed: statusFeed; menuModel: menuState }
    RecordingOverlay {
        feed: statusFeed
        targetScreen: root.QsWindow.window ? root.QsWindow.window.screen : Quickshell.screens[0] || null
    }

    Component.onCompleted: menuState.refresh()
    Connections {
        target: statusFeed
        function onPhaseChanged(): void {
            if (statusFeed.phase === "disconnected" && !menuState.busy) root.close();
        }
    }

    Ui.BarIconButton {
        id: button
        anchors.fill: parent
        bar: root.bar
        text: statusFeed.phase === "recording" ? "●" : statusFeed.phase === "transcribing" ? "◌" : ""
        iconComponent: ["recording", "transcribing"].includes(statusFeed.phase) ? null : speakIcon
        active: statusFeed.phase === "recording"
        tooltipText: root.opened ? "" : "JustSpeak · " + statusFeed.label
        onPressed: mouseButton => {
            if (mouseButton === Qt.RightButton && statusFeed.canCancel) menuState.run(["cancel"], "Canceled");
            else root.toggle();
        }
    }

    Component {
        id: speakIcon
        JustSpeakIcon { color: button.foreground }
    }

    DictationMenu {
        id: dictationMenu
        anchorItem: button
        bar: root.bar
        owner: root
        open: root.opened
        feed: statusFeed
        menuModel: menuState
        updateModel: updateState
    }
}
