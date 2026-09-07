import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import "App" as App

ShellRoot {
    id: test
    property string failure: Quickshell.env("HARNESS_FAILURE") || ""
    property bool compositor: Quickshell.env("HARNESS_COMPOSITOR") === "1"
    property var recorder: null
    property var capture: recorder ? recorder.capture : null
    property var steps: []
    property int stage: 0
    property double stageStarted: Date.now()
    function check(condition, message) { if (!condition) throw new Error(message); }
    function find(node, predicate) {
        if (predicate(node)) return node;
        for (const child of node.children || []) { const result = find(child, predicate); if (result) return result; }
        return null;
    }
    function key(key, code, mods = 0) { return {key:key, nativeScanCode:code, modifiers:mods, isAutoRepeat:false, accepted:false}; }
    function press(keycode = Qt.Key_F10, code = 76) { capture.press(key(keycode, code)); }
    function release(keycode = Qt.Key_F10, code = 76) { capture.release(key(keycode, code)); }
    function begin() {
        find(recorder, node => node.text === "Record shortcut…").clicked();
        if (!compositor && failure !== "denied") capture.protectedInput = true;
        check(panel.open && recorder.editing, "Recorder left the popup");
    }
    App.StatusFeed { id: feed }
    App.MenuModel { id: model; feed: feed }
    App.UpdateModel { id: updates; feed: feed; menuModel: model }
    Process { id: typer }
    PanelWindow {
        id: anchorWindow
        visible: test.compositor
        screen: Quickshell.screens[0] || null
        implicitWidth: 24; implicitHeight: 24
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "just-speak-inline-test-anchor"
        anchors { top: true; right: true }
        Item { id: anchor; width: 24; height: 24 }
    }
    QtObject {
        id: bar
        property string position: "top"
        property int barSize: 32
        property var activePopout: null
        function requestPopout(owner) { activePopout = owner; }
        function releasePopout(owner) { activePopout = null; }
    }
    App.DictationMenu {
        id: panel
        visible: test.compositor && open
        feed: feed; menuModel: model; updateModel: updates; anchorItem: anchor; bar: bar
    }
    PanelWindow {
        id: otherFocus
        visible: false
        screen: anchorWindow.screen
        implicitWidth: 1; implicitHeight: 1
        color: "transparent"
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.namespace: "just-speak-inline-test-focus"
        WlrLayershell.layer: WlrLayer.Overlay
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.Exclusive
    }
    Component.onCompleted: {
        model.refresh();
        steps = [
            function() {
                if (!model.ready || !feed.modelReady) return false;
                for (const item of panel.contentItem) {
                    const result = find(item, node => node.objectName === "shortcutRecorder");
                    if (result) recorder = result;
                }
                check(recorder && recorder.available, "Missing enabled inline recorder");
                feed.phase = "recording"; return true;
            },
            function() { check(!recorder.available, "Dictation did not disable recording"); feed.phase = "idle"; updates.installing = true; return true; },
            function() { check(!recorder.available, "Update did not disable recording"); updates.installing = false;
                model.data = {settings:{shortcut:"CTRL + F11"},history:[],inputs:[],desktop:{shortcut_editing:false}}; return true; },
            function() { check(!recorder.available, "Unsupported desktop allowed recording");
                model.data = {settings:{shortcut:"CTRL + F11"},history:[],inputs:[],desktop:{shortcut_editing:true}}; panel.open = true; return true; },
            function() { begin(); return true; },
            function() { if (!capture.protectedInput) return false;
                check(!panel.editable, "Other settings still editable during capture"); press(); return true; },
            function() { if (capture.phase !== "preview") return false;
                check(capture.candidate === "F10" && !capture.canSave, "Bad capture or held-key gate"); release(); return true; },
            function() { check(capture.canSave, "Release did not enable Save"); capture.commit(); return true; },
            function() { if (capture.saving) return false;
                check(capture.editing && capture.error.includes("Fixture conflict"), "Save failure discarded preview");
                capture.retry(); press(); return true; },
            function() { if (capture.phase !== "preview") return false; release(); capture.commit(); return true; },
            function() { if (capture.editing || model.loading) return false;
                check(panel.open && model.data.settings.shortcut === "F10", "Save closed popup or did not refresh shortcut"); begin(); return true; },
            function() { if (!capture.protectedInput) return false; press(); return true; },
            function() { if (capture.phase !== "preview") return false; release();
                if (compositor) otherFocus.visible = true; else capture.protectedInput = false; return true; },
            function() { if (capture.protectedInput) return false;
                check(capture.candidate === "F10" && !capture.canSave, "Focus loss discarded preview");
                if (compositor) { otherFocus.visible = false; panel.focusPrimed = false; panel.beginFocusPrime(); }
                else capture.protectedInput = true;
                return true; },
            function() { if (!capture.protectedInput) return false;
                check(capture.canSave, "Preview did not resume");
                capture.retry(); press(); panel.close();
                check(panel.open && capture.pendingClose, "Outside dismissal released held keys"); release(); return true; },
            function() { check(!panel.open && !capture.editing, "Pending close did not finish after release");
                panel.open = true; begin(); return true; },
            function() { if (!capture.protectedInput) return false;
                press(Qt.Key_Escape, 9); check(capture.editing, "Escape did not wait for release");
                release(Qt.Key_Escape, 9); check(!capture.editing && panel.open, "Escape closed entire popup"); return true; },
            function() { if (compositor) begin(); return true; },
            function() {
                if (!compositor) return true;
                if (!capture.protectedInput) return false;
                typer.command = ["wtype", "-P", "F35", "-s", "250", "-p", "F35"];
                typer.running = true; return true;
            },
            function() {
                if (!compositor) return true;
                if (!capture.canSave || typer.running) return false;
                check(capture.candidate === "F35", "Real key event translation failed");
                panel.contentItem[0].grabToImage(result => result.saveToFile(Quickshell.env("HARNESS_ROOT") + "/preview.png"));
                typer.command = ["wtype", "-P", "F34", "-s", "500", "-p", "F34"];
                typer.running = true; return true;
            },
            function() {
                if (!compositor) return true;
                if (!capture.keysDown) return false;
                check(!capture.canSave && capture.candidate === "F35", "Real held preview key enabled Save");
                capture.cancel(); check(capture.pendingClose, "Cancel lost protection before actual release"); return true;
            },
            function() { if (compositor && (capture.editing || typer.running)) return false; return true; },
            function() { if (compositor) begin(); return true; },
            function() {
                if (!compositor) return true;
                if (!capture.protectedInput) return false;
                typer.command = ["wtype", "-P", "Super_L", "-M", "logo", "-P", "F11", "-s", "250", "-p", "F11", "-m", "logo", "-p", "Super_L"];
                typer.running = true; return true;
            },
            function() {
                if (!compositor) return true;
                if (!capture.canSave || typer.running) return false;
                check(capture.candidate === "SUPER + F11", "Super+F11 captured incorrectly: " + capture.candidate);
                find(recorder, node => node.text === "Save").clicked(); return true;
            },
            function() {
                if (!compositor) return true;
                if (capture.editing || model.loading) return false;
                check(panel.open && model.data.settings.shortcut === "SUPER + F11", "Super+F11 did not save in place"); return true;
            },
            function() { console.log("INLINE_PASS", compositor ? "installed plugin layout; actual Super+F11 capture and Save; focus and release gating" : "isolated UI"); panel.open = false; Qt.quit(); return true; }
        ];
        if (failure) {
            steps = [steps[0], function() { feed.phase = "idle"; panel.open = true; begin(); return true; },
                function() {
                    if (failure === "denied") return true;
                    if (!capture.protectedInput) return false;
                    if (failure === "capture-timeout") press(Qt.Key_Shift, 50);
                    else press();
                    return true;
                },
                function() {
                    if (failure === "denied") {
                        if (capture.editing) return false;
                        check(model.error.includes("protection"), "Denied inhibitor did not explain failure");
                        return true;
                    }
                    if (failure === "capture-timeout") {
                        if (!capture.pendingClose) return false;
                        check(capture.editing && model.error.includes("timed out"), "Timeout released held keys");
                        release(Qt.Key_Shift, 50); check(!capture.editing, "Timeout cancel did not finish"); return true;
                    }
                    if (capture.phase !== "recording" || !capture.error) return false;
                    if (failure === "lookup-timeout") check(capture.error.includes("timed out"), "Expected a lookup timeout: " + capture.error);
                    release(); check(!capture.canSave, "Failed key lookup enabled Save");
                    press(); return true;
                },
                function() {
                    if (!["denied", "capture-timeout"].includes(failure)) {
                        if (capture.phase !== "preview") return false;
                        release(); check(capture.canSave, "Lookup could not recover after failure"); capture.cancel();
                    }
                    check(panel.open && model.data.settings.shortcut === "CTRL + F11", "Failure changed shortcut or left popup");
                    console.log("INLINE_PASS", failure); panel.open = false; Qt.quit(); return true;
                }
            ];
        }
    }
    Timer {
        interval: 20; running: true; repeat: true
        onTriggered: {
            try {
                if (Date.now() - test.stageStarted > (test.failure === "capture-timeout" ? 35000 : 6000)) throw new Error("Stage timeout " + test.stage + ": " + (test.capture ? test.capture.phase + " " + test.capture.error : "not loaded"));
                if (test.stage < test.steps.length && test.steps[test.stage]()) {
                    console.log("INLINE_STEP", test.stage++); test.stageStarted = Date.now();
                }
            } catch (error) { console.error("INLINE_FAIL", error.message); panel.open = false; otherFocus.visible = false; Qt.exit(1); }
        }
    }
}
