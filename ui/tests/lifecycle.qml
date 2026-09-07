import QtQuick
import Quickshell
import Quickshell.Wayland
import "App" as App

ShellRoot {
    id: test
    property var steps: []
    property int stage: 0
    property int ticks: 0
    property var quitButton: null
    property var widgetFeed: null
    function check(ok, message) { if (!ok) throw new Error(message); }
    function find(node) {
        if (node.text === "Quit") return node;
        for (const child of node.children || []) { const found = find(child); if (found) return found; }
        return null;
    }
    App.StatusFeed { id: feed }
    App.MenuModel { id: model; feed: feed }
    App.UpdateModel { id: updates; feed: feed; menuModel: model }
    QtObject {
        id: bar
        property string position: "top"
        property int barSize: 32
        property bool vertical: false
        property string fontFamily: "sans-serif"
        property color barForeground: "white"
        property color urgent: "red"
        property bool foregroundAnimationEnabled: false
        property var activePopout: null
        function requestPopout(owner) { activePopout = owner; }
        function releasePopout(owner) { activePopout = null; }
        function hideTooltip(owner) {}
    }
    PanelWindow {
        // An input-transparent 1px surface gives the widget a visible parent
        // without drawing UI or taking keyboard/pointer focus.
        visible: true
        implicitWidth: 1; implicitHeight: 1
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        mask: Region {}
        WlrLayershell.namespace: "just-speak-lifecycle-test"
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
        Item { id: anchor; width: 24; height: 24 }
        App.Widget { id: widget; opacity: 0; bar: bar }
    }
    App.DictationMenu {
        id: panel
        visible: false
        feed: feed; menuModel: model; updateModel: updates; anchorItem: anchor; bar: bar
    }
    Component.onCompleted: {
        for (const child of widget.data) {
            if ("modelReady" in child && "phase" in child) widgetFeed = child;
        }
        steps = [
            function() {
                if (!feed.modelReady || !widget.visible || widget.implicitWidth <= 0) return false;
                for (const item of panel.contentItem) { const result = find(item); if (result) quitButton = result; }
                check(quitButton && quitButton.enabled, "Quit unavailable while idle");
                panel.open = true;
                quitButton.clicked();
                check(panel.open && model.busy, "Quit closed popup before its reply");
                return true;
            },
            function() {
                if (model.busy) return false;
                check(panel.open && model.error.includes("Fixture") && widget.visible, "Failed Quit hid its error or icon");
                feed.phase = "updating"; return true;
            },
            function() { check(!quitButton.enabled, "Quit allowed during update"); feed.phase = "loading"; return true; },
            function() { check(quitButton.enabled, "Quit blocked during loading"); feed.phase = "recording"; return true; },
            function() { check(quitButton.enabled, "Quit blocked during recording"); feed.phase = "transcribing"; return true; },
            function() {
                check(quitButton.enabled, "Quit blocked during transcription");
                quitButton.clicked();
                check(panel.open && model.busy && !model.error, "Retry failed to enter pending state");
                return true;
            },
            function() {
                if (model.busy || widget.visible) return false;
                check(!panel.open && !model.error && widget.implicitWidth === 0 && widget.implicitHeight === 0,
                      "Successful Quit left popup, error or bar space behind");
                model.refresh(); return true;
            },
            function() {
                check(!model.loading && !model.error, "Quit triggered a failing menu refresh");
                model.run(["launch"], "Starting JustSpeak"); return true;
            },
            function() {
                if (model.busy || !widget.visible || !feed.modelReady) return false;
                check(widget.implicitWidth > 0, "Launch did not restore the icon"); return true;
            },
        ];
    }
    Timer {
        interval: 50; repeat: true; running: true
        onTriggered: {
            try {
                test.check(test.ticks++ < 200, "Lifecycle test timed out at step " + test.stage
                    + ": phase=" + feed.phase + " ready=" + feed.modelReady
                    + " visible=" + widget.visible + " width=" + widget.implicitWidth
                    + " widgetPhase=" + (test.widgetFeed ? test.widgetFeed.phase : "missing"));
                if (!test.steps[test.stage]()) return;
                test.stage++; test.ticks = 0;
                if (test.stage === test.steps.length) { console.log("INLINE_PASS lifecycle"); Qt.quit(); }
            } catch (error) { console.error(error.message); Qt.exit(1); }
        }
    }
}
