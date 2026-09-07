import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import System from 'system';
import {ShortcutRecorder} from '../shortcut-recorder.js';

Gtk.init();
Gio._promisify(Gio.Subprocess.prototype, 'wait_check_async');
const loop = new GLib.MainLoop(null, false);
let exitCode = 0;
let liveRecorder = null;
const parent = new Gtk.Window({title: 'JustSpeak shortcut test', default_width: 440, default_height: 100});
parent.set_child(new Gtk.Label({label: 'Synthetic shortcut test. No microphone, clipboard, or configuration access.'}));
function assert(value, message) { if (!value) throw new Error(message); }
function sleep(ms) {
    return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, ms, () => { resolve(); return GLib.SOURCE_REMOVE; }));
}
async function until(predicate, message, timeout = 2000) {
    const end = Date.now() + timeout;
    while (!predicate()) { if (Date.now() > end) throw new Error(message); await sleep(10); }
}
function make(options = {}) {
    liveRecorder = new ShortcutRecorder({parent, current: 'SUPER + F10', save: async () => {}, closed: () => {}, ...options});
    return liveRecorder;
}
function preview(recorder) {
    recorder.capture.arm();
    recorder.capture.press('F12', 96, 0);
    recorder.capture.release('F12', 96);
    recorder.render();
}
function protectedFixture(recorder) {
    const realWindow = recorder.window;
    const fixture = {restored: 0, navigation: []};
    recorder.window = {is_active: true, destroy: () => realWindow.destroy(),
        set_transient_for: value => realWindow.set_transient_for(value),
        child_focus: direction => { fixture.navigation.push(direction); return true; }, get_focus: () => null};
    recorder.surface = {shortcuts_inhibited: true, restore_system_shortcuts() { fixture.restored++; }};
    recorder.inhibitRequested = true;
    return fixture;
}
async function run() {
    let restored = 0;
    let closed = 0;
    let recorder = make({closed: () => closed++});
    recorder.surface = {shortcuts_inhibited: false, restore_system_shortcuts() { restored++; }};
    recorder.inhibitRequested = true;
    recorder.keyPressed(Gdk.KEY_F10, 76, 0);
    assert(recorder.capture.candidate === '', 'Waiting dialog accepted a shortcut before inhibition');
    recorder.capture.arm();
    recorder.keyPressed(Gdk.KEY_F10, 76, 0);
    assert(recorder.closed && restored === 1, 'Revoked inhibition did not abort and restore');
    recorder.close();
    assert(closed === 1 && restored === 1, 'Cleanup was not idempotent');

    recorder = make();
    recorder.surface = {restore_system_shortcuts() { restored++; }};
    recorder.inhibitRequested = true;
    recorder.hadFocus = true;
    preview(recorder);
    recorder.focusChanged(false);
    assert(recorder.closed && recorder.capture.held.size === 0 && restored === 2, 'Focus loss did not discard preview and restore');

    let stored = 'SUPER + F10';
    recorder = make({save: async () => { throw new Error('F12 is already assigned to another application'); }});
    preview(recorder);
    await recorder.commit();
    assert(!recorder.closed && recorder.capture.candidate === 'F12' && recorder.error.visible,
        'Conflict did not retain the captured preview and explain the failure');
    assert(stored === 'SUPER + F10', 'Conflict changed the existing shortcut');
    recorder.close();

    let finishSave;
    recorder = make({save: value => new Promise(resolve => { finishSave = () => { stored = value; resolve(); }; })});
    preview(recorder);
    const pendingSave = recorder.commit();
    recorder.keyPressed(Gdk.KEY_Escape, 9, 0);
    recorder.cancelButton.emit('clicked');
    assert(recorder.saving && !recorder.closed && !recorder.cancelButton.sensitive, 'Escape/Cancel pretended to abort a committed save');
    finishSave();
    await pendingSave;
    assert(recorder.closed && stored === 'F12', 'Explicit save did not finish');

    recorder = make();
    let protection = protectedFixture(recorder);
    recorder.capture.arm();
    recorder.capture.press('F12', 96, Gdk.ModifierType.SUPER_MASK);
    recorder.keyReleased(Gdk.KEY_F12, 96, Gdk.ModifierType.SUPER_MASK);
    recorder.close();
    assert(!recorder.closed && recorder.pendingClose && protection.restored === 0, 'Cancel restored while a pre-held modifier was still down');
    recorder.keyReleased(Gdk.KEY_Super_L, 133, Gdk.ModifierType.SUPER_MASK);
    assert(recorder.closed && protection.restored === 1, 'Cancel did not complete after the pre-held modifier released');

    recorder = make(); protection = protectedFixture(recorder); preview(recorder);
    recorder.keyPressed(Gdk.KEY_F34, 194, 0);
    recorder.keyPressed(Gdk.KEY_Escape, 9, 0);
    recorder.keyReleased(Gdk.KEY_F34, 194, 0);
    assert(!recorder.closed && protection.restored === 0, 'Escape restored before Escape itself was released');
    recorder.keyReleased(Gdk.KEY_Escape, 9, 0);
    assert(recorder.closed && protection.restored === 1, 'Deferred Escape did not restore on final release');

    recorder = make({save: () => new Promise(resolve => { finishSave = resolve; })});
    protection = protectedFixture(recorder); preview(recorder);
    const heldSave = recorder.commit();
    recorder.keyPressed(Gdk.KEY_F34, 194, 0);
    finishSave(); await heldSave;
    assert(!recorder.closed && recorder.pendingClose && protection.restored === 0, 'Completed save restored while a newly pressed key was held');
    recorder.keyReleased(Gdk.KEY_F34, 194, 0);
    assert(recorder.closed && protection.restored === 1, 'Saved dialog did not close after final key release');

    recorder = make(); protection = protectedFixture(recorder); preview(recorder);
    recorder.keyPressed(Gdk.KEY_Tab, 23, 0); recorder.keyReleased(Gdk.KEY_Tab, 23, 0);
    recorder.keyPressed(Gdk.KEY_Shift_L, 50, 0);
    recorder.keyPressed(Gdk.KEY_ISO_Left_Tab, 23, Gdk.ModifierType.SHIFT_MASK);
    recorder.keyReleased(Gdk.KEY_ISO_Left_Tab, 23, Gdk.ModifierType.SHIFT_MASK);
    recorder.keyReleased(Gdk.KEY_Shift_L, 50, Gdk.ModifierType.SHIFT_MASK);
    assert(protection.navigation.join(',') === [Gtk.DirectionType.TAB_FORWARD, Gtk.DirectionType.TAB_BACKWARD].join(','),
        'Tab and Shift+Tab did not navigate after all keys were released');
    recorder.close();

    let notice = '';
    recorder = make({timeout: 20, closed: message => { notice = message; }});
    recorder.startTimeout();
    await until(() => recorder.closed, 'Recorder timeout did not close');
    assert(notice.includes('timed out') && recorder.timer === 0, 'Timeout did not clean up and explain cancellation');
    print('SHORTCUT_WINDOW_TESTS_COMPLETE: GTK constructor, protection, focus loss, conflict, deferred cancel/save, pre-held modifiers, navigation, timeout');

    if (ARGV.includes('--compositor')) {
        // Run explicitly on a compositor advertising keyboard-shortcuts-inhibit.
        // F34/F35 must first be verified unbound; neither starts dictation.
        parent.present();
        await until(() => parent.is_active, 'Synthetic parent window did not gain compositor focus', 4500);
        let saves = 0;
        recorder = make({save: async value => { assert(value === 'F35', 'Actual key mapped incorrectly'); saves++; }});
        recorder.present();
        await until(() => recorder.capture.phase === 'recording', 'Compositor did not grant a focused shortcut inhibitor', 4500);
        assert(recorder.surface.shortcuts_inhibited && recorder.window.is_active, 'Recorder armed without focus and inhibition');
        const first = Gio.Subprocess.new(['wtype', '-k', 'F35'], Gio.SubprocessFlags.NONE);
        await first.wait_check_async(null);
        await until(() => recorder.capture.canSave, 'Actual F35 key events did not reach recorder');
        assert(recorder.capture.candidate === 'F35', 'Actual F35 keysym did not produce F35');
        const second = Gio.Subprocess.new(['wtype', '-P', 'F34', '-s', '250', '-p', 'F34'], Gio.SubprocessFlags.NONE);
        await until(() => recorder.capture.held.size > 0, 'Preview did not track a subsequently held key');
        assert(!recorder.saveButton.sensitive && recorder.surface.shortcuts_inhibited, 'Held preview key lost inhibition or enabled Save');
        await recorder.commit();
        assert(saves === 0, 'Held preview key allowed a save');
        await second.wait_check_async(null);
        await until(() => recorder.capture.canSave, 'Preview key release was lost');
        const surface = recorder.surface;
        await recorder.commit();
        assert(saves === 1 && recorder.closed && !recorder.inhibitRequested, 'Actual inhibitor was not cleaned up after Save');
        assert(!surface.shortcuts_inhibited, 'GDK still reported inhibition after cleanup');
        print('SHORTCUT_COMPOSITOR_TEST_COMPLETE: confirmed active inhibitor, actual unbound F35, held F34 safety, release, explicit fake Save, restored inhibitor');
    }
}
run().catch(error => { exitCode = 1; printerr(`${error.message}\n${error.stack || ''}`); }).finally(() => {
    liveRecorder?.close('', true); parent.destroy(); loop.quit();
});
loop.run();
System.exit(exitCode);
