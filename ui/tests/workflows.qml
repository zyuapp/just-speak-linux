import QtQuick
import Quickshell
import Quickshell.Io
import Quickshell.Wayland
import "App" as App

ShellRoot {
    id: test
    property var steps: []
    property int stage: 0
    property double stageStarted: Date.now()
    property var startButton: null
    property var installButton: null
    property string validExecutable: ""
    function check(ok, message) { if (!ok) throw new Error(message); }
    function find(node, predicate) {
        if (predicate(node)) return node;
        for (const child of node.children || []) { const result = find(child, predicate); if (result) return result; }
        return null;
    }
    function button(text) {
        for (const item of panel.contentItem) {
            const result = find(item, node => node.text === text);
            if (result) return result;
        }
        throw new Error("Missing button: " + text);
    }
    function configure(options) {
        controller.command = [feed.executable, "fixture", JSON.stringify(options)];
        controller.running = true;
    }
    App.StatusFeed { id: feed }
    App.MenuModel { id: model; feed: feed }
    App.UpdateModel { id: updates; feed: feed; menuModel: model }
    Process { id: controller }
    PanelWindow {
        visible: false
        implicitWidth: 24; implicitHeight: 24
        exclusionMode: ExclusionMode.Ignore
        WlrLayershell.keyboardFocus: WlrKeyboardFocus.None
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
        visible: false
        feed: feed; menuModel: model; updateModel: updates; anchorItem: anchor; bar: bar
    }
    Component.onCompleted: {
        steps = [
            function() {
                if (!model.canEdit) return false;
                startButton = button("Start dictation"); installButton = button("Install update");
                configure({menu_delay:0.3}); return true;
            },
            function() {
                model.refresh();
                check(model.loading && !model.canEdit, "Settings stayed editable while their snapshot was loading");
                model.run(["settings", "set", "paste", "true"], "Settings saved");
                model.refresh(); check(model.refreshPending, "Refresh overlapped a mutation"); return true;
            },
            function() {
                if (model.busy || model.loading || model.refreshPending) return false;
                check(model.ready && model.data.settings.paste === true, "Pre-action snapshot overwrote the saved setting");
                configure({menu_malformed:true}); return true;
            },
            function() { model.refresh(); return true; },
            function() {
                if (model.loading) return false;
                check(!model.canEdit && model.error.includes("Could not read"), "Malformed menu response left stale settings enabled");
                configure({menu_delay:3}); return true;
            },
            function() { button("Refresh").clicked(); return true; },
            function() {
                if (model.loading) return false;
                check(model.error.includes("timed out"), "Settings timeout was replaced by an exit error");
                configure({}); return true;
            },
            function() { button("Refresh").clicked(); return true; },
            function() {
                if (model.loading) return false;
                check(model.canEdit && !model.error, "Settings refresh did not recover");
                configure({action_error:true}); return true;
            },
            function() {
                panel.open = true; startButton.clicked();
                check(!panel.open && panel.outsideActionPending, "Start did not release popup focus before capturing the target"); return true;
            },
            function() {
                if (panel.outsideActionPending) return false;
                check(panel.open && model.error.includes("microphone is unavailable"), "Failed Start hid its error");
                panel.outsideAction(["history", "paste", "7"]); return true;
            },
            function() {
                if (panel.outsideActionPending) return false;
                check(panel.open && model.error.includes("paste target disappeared"), "Failed Paste hid its error");
                configure({}); return true;
            },
            function() { startButton.clicked(); return true; },
            function() {
                if (panel.outsideActionPending || model.loading) return false;
                check(!panel.open && !model.error, "Successful Start reopened the popup");
                panel.open = true; button("Copy").clicked(); return true;
            },
            function() {
                if (model.busy || model.loading) return false;
                check(panel.open && model.notice === "Copied to clipboard", "Copy failed to give visible feedback");
                feed.update(JSON.stringify({phase:"recording", model_ready:true, can_cancel:false}));
                check(!feed.canCancel, "Status ignored explicit cancellation denial");
                feed.update(JSON.stringify({phase:"canceling", model_ready:true, can_cancel:false})); return true;
            },
            function() {
                check(feed.phase === "canceling" && feed.label.includes("Canceling"), "Canceling state was ignored");
                check(!model.canEdit && !startButton.enabled && model.canQuit, "Canceling gates disagree with cleanup");
                feed.update(JSON.stringify({phase:"error", model_ready:true, message:"Fixture: speech worker failed"})); return true;
            },
            function() {
                if (model.loading) return false;
                check(button("Fixture: speech worker failed").visible, "Daemon error detail was hidden from the menu");
                feed.update(JSON.stringify({phase:"idle", model_ready:true}));
                configure({action_delay:3}); return true;
            },
            function() { model.run(["restart"], "Restarting"); return true; },
            function() {
                if (model.busy || model.loading) return false;
                check(model.error.includes("timed out") && !model.notice, "Action timeout lost its error or kept a success notice");
                configure({check_delay:3}); return true;
            },
            function() { updates.check(true); return true; },
            function() {
                if (updates.checking) return false;
                check(updates.error.includes("timed out"), "Update timeout was replaced by an exit error");
                configure({check_malformed:true}); return true;
            },
            function() { updates.check(true); return true; },
            function() {
                if (updates.checking) return false;
                check(!updates.available && updates.error.includes("Could not read"), "Malformed update enabled Install");
                configure({check_error:true}); return true;
            },
            function() { updates.check(true); return true; },
            function() {
                if (updates.checking) return false;
                check(updates.error.includes("server is unavailable"), "Update check hid the backend failure");
                configure({install_error:true}); return true;
            },
            function() { updates.check(true); return true; },
            function() {
                if (updates.checking) return false;
                check(updates.available && !updates.error && updates.latestVersion === "9.9.9", "Update check failed to recover");
                installButton.clicked(); return true;
            },
            function() {
                if (updates.installing) return false;
                check(updates.available && updates.error.includes("download failed") && installButton.enabled,
                    "Failed update discarded retry or error");
                configure({}); return true;
            },
            function() { installButton.clicked(); return true; },
            function() {
                if (updates.installing) return false;
                check(!updates.available && !updates.error && updates.notice.includes("installed"), "Update retry did not report completion");
                validExecutable = feed.executable;
                feed.executable = "/just-speak-fixture-missing-executable";
                model.run(["restart"], "Restarting"); return true;
            },
            function() {
                if (model.busy) return false;
                check(model.error.includes("Could not start"), "Missing executable did not report an action error");
                updates.check(true); return true;
            },
            function() {
                if (updates.checking) return false;
                check(updates.error.includes("Could not start"), "Missing executable did not report a check error");
                updates.available = true; updates.install(); return true;
            },
            function() {
                if (updates.installing) return false;
                check(updates.error.includes("Could not start"), "Missing executable did not report an install error");
                feed.executable = validExecutable;
                button("Refresh").clicked(); updates.check(true); return true;
            },
            function() {
                if (model.loading || updates.checking) return false;
                check(model.canEdit && !model.error && updates.available && !updates.error,
                    "Commands did not recover after the executable became available");
                console.log("INLINE_PASS workflows"); Qt.quit(); return true;
            }
        ];
    }
    Timer {
        interval: 20; running: true; repeat: true
        onTriggered: {
            try {
                if (Date.now() - test.stageStarted > 6000) throw new Error("Workflow timeout at step " + test.stage
                    + ": " + model.error + " " + updates.error);
                if (controller.running) return;
                if (test.steps[test.stage]()) {
                    console.log("INLINE_STEP", test.stage++); test.stageStarted = Date.now();
                }
            } catch (error) { console.error("INLINE_FAIL", error.message); Qt.exit(1); }
        }
    }
}
