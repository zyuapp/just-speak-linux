import QtQuick
import QtTest
import ".." as App

TestCase {
    name: "InlineShortcutCapture"
    App.ShortcutCapture { id: capture }
    SignalSpy { id: saves; target: capture; signalName: "saveRequested" }
    SignalSpy { id: resolutions; target: capture; signalName: "resolveRequested" }
    function event(key, code, mods = 0, repeat = false) {
        return {key: key, nativeScanCode: code, modifiers: mods, isAutoRepeat: repeat, accepted: false};
    }
    function press(key, code, mods = 0, repeat = false) { capture.press(event(key, code, mods, repeat)); }
    function release(key, code, mods = 0, repeat = false) { capture.release(event(key, code, mods, repeat)); }
    function resolve(key) { capture.resolved(capture.generation, key, ""); }
    function preview() {
        press(Qt.Key_F10, 76); resolve("F10"); release(Qt.Key_F10, 76);
        compare(capture.candidate, "F10"); verify(capture.canSave);
    }
    function init() {
        capture.finish(); capture.protectedInput = true; capture.begin();
        saves.clear(); resolutions.clear();
    }
    function test_waits_for_protection() {
        capture.protectedInput = false;
        press(Qt.Key_F10, 76); compare(resolutions.count, 0);
        verify(!capture.canSave); compare(capture.phase, "waiting");
        capture.protectedInput = true; preview();
    }
    function test_function_and_release() {
        press(Qt.Key_F10, 76); resolve("F10"); verify(!capture.canSave);
        capture.commit(); compare(saves.count, 0);
        release(Qt.Key_F10, 76); capture.commit();
        compare(saves.count, 1); compare(saves.signalArguments[0][0], "F10");
        capture.commit(); compare(saves.count, 1);
    }
    function test_chords_data() {
        return [
            {tag: "super", modifier: Qt.Key_Meta, mask: Qt.MetaModifier, expected: "SUPER + F10"},
            {tag: "control", modifier: Qt.Key_Control, mask: Qt.ControlModifier, expected: "CTRL + F10"},
            {tag: "alt", modifier: Qt.Key_Alt, mask: Qt.AltModifier, expected: "ALT + F10"},
            {tag: "shift", modifier: Qt.Key_Shift, mask: Qt.ShiftModifier, expected: "SHIFT + F10"}
        ];
    }
    function test_chords(data) {
        press(data.modifier, 50, data.mask); verify(capture.error !== "");
        press(Qt.Key_F10, 76, data.mask); resolve("F10");
        compare(capture.candidate, data.expected);
        release(Qt.Key_F10, 76, data.mask); verify(!capture.canSave);
        release(data.modifier, 50, data.mask); verify(capture.canSave);
    }
    function test_preheld_and_multiple_modifiers() {
        const masks = Qt.MetaModifier | Qt.ControlModifier | Qt.AltModifier | Qt.ShiftModifier;
        press(Qt.Key_F10, 76, masks); resolve("F10");
        compare(capture.candidate, "SUPER + CTRL + ALT + SHIFT + F10");
        release(Qt.Key_F10, 76, masks); verify(!capture.canSave);
        release(Qt.Key_Meta, 133, masks);
        release(Qt.Key_Control, 37, masks & ~Qt.MetaModifier);
        release(Qt.Key_Alt, 64, Qt.AltModifier | Qt.ShiftModifier);
        release(Qt.Key_Shift, 50, Qt.ShiftModifier); verify(capture.canSave);
    }
    function test_two_shifts() {
        press(Qt.Key_Shift, 50, Qt.ShiftModifier); press(Qt.Key_Shift, 62, Qt.ShiftModifier);
        press(Qt.Key_F10, 76, Qt.ShiftModifier); resolve("F10");
        release(Qt.Key_F10, 76, Qt.ShiftModifier);
        release(Qt.Key_Shift, 50, Qt.ShiftModifier); verify(!capture.canSave);
        release(Qt.Key_Shift, 62, Qt.ShiftModifier); verify(capture.canSave);
    }
    function test_shifted_punctuation() {
        press(Qt.Key_Exclam, 10, Qt.ControlModifier | Qt.ShiftModifier); resolve("1");
        compare(capture.candidate, "CTRL + SHIFT + 1");
    }
    function test_invalid_retry_data() {
        return [
            {tag: "typing", key: Qt.Key_A, base: "A", mods: 0},
            {tag: "unsupported", key: Qt.Key_F10, base: "", mods: 0},
            {tag: "iso", key: Qt.Key_F10, base: "ISO_LEVEL3_SHIFT", mods: Qt.ControlModifier},
            {tag: "invalid-name", key: Qt.Key_F10, base: "bad;name", mods: Qt.ControlModifier}
        ];
    }
    function test_invalid_retry(data) {
        press(data.key, 38, data.mods); resolve(data.base); release(data.key, 38);
        compare(capture.phase, "recording"); verify(capture.error !== "");
        compare(capture.candidate, ""); preview();
    }
    function test_altgr_and_keypad() {
        press(Qt.Key_AltGr, 108); compare(resolutions.count, 0); release(Qt.Key_AltGr, 108);
        press(Qt.Key_E, 26, Qt.GroupSwitchModifier); compare(resolutions.count, 0); release(Qt.Key_E, 26);
        press(Qt.Key_1, 87, Qt.KeypadModifier | Qt.ControlModifier); compare(resolutions.count, 0);
        verify(capture.error !== "");
    }
    function test_repeat_and_extra_preview_keys() {
        press(Qt.Key_F10, 76); press(Qt.Key_F10, 76, 0, true); compare(resolutions.count, 1);
        resolve("F10"); release(Qt.Key_F10, 76, 0, true); verify(!capture.canSave);
        release(Qt.Key_F10, 76); verify(capture.canSave);
        press(Qt.Key_F11, 95); verify(!capture.canSave); compare(capture.candidate, "F10");
        release(Qt.Key_F11, 95); verify(capture.canSave);
    }
    function test_cancel_waits_for_all_releases() {
        press(Qt.Key_Meta, 133, Qt.MetaModifier); press(Qt.Key_F10, 76, Qt.MetaModifier); resolve("F10");
        press(Qt.Key_Escape, 9, Qt.MetaModifier); verify(capture.pendingClose); verify(capture.editing);
        release(Qt.Key_Escape, 9, Qt.MetaModifier); release(Qt.Key_F10, 76, Qt.MetaModifier);
        verify(capture.editing); release(Qt.Key_Meta, 133, Qt.MetaModifier);
        verify(!capture.editing); compare(saves.count, 0);
    }
    function test_cancel_focus_loss() {
        press(Qt.Key_F10, 76); capture.cancel(); verify(capture.editing);
        capture.protectedInput = false; verify(!capture.editing);
    }
    function test_preview_survives_focus_loss() {
        preview(); capture.protectedInput = false;
        compare(capture.candidate, "F10"); verify(!capture.canSave);
        press(Qt.Key_F11, 95); compare(capture.candidate, "F10");
        capture.protectedInput = true; verify(capture.canSave);
    }
    function test_held_focus_loss_requires_recapture() {
        preview(); press(Qt.Key_F11, 95); capture.protectedInput = false;
        compare(capture.candidate, ""); verify(capture.error !== "");
        release(Qt.Key_F11, 95); verify(!capture.canSave);
        capture.protectedInput = true; preview();
    }
    function test_stale_resolution() {
        press(Qt.Key_F10, 76); const old = capture.generation;
        capture.protectedInput = false; capture.protectedInput = true;
        press(Qt.Key_F11, 95); capture.resolved(old, "F10", "");
        compare(capture.candidate, ""); resolve("F11"); compare(capture.candidate, "F11");
    }
    function test_save_failure_retry() {
        preview(); capture.commit(); capture.saved(false, "F10 is occupied");
        compare(capture.error, "F10 is occupied"); verify(capture.editing); verify(capture.canSave);
        capture.retry(); preview(); capture.commit(); capture.saved(true, ""); verify(!capture.editing);
    }
    function test_keys_during_save() {
        preview(); capture.commit(); press(Qt.Key_F11, 95); capture.cancel(); verify(capture.saving);
        capture.saved(true, ""); verify(capture.pendingClose); verify(capture.editing);
        release(Qt.Key_F11, 95); verify(!capture.editing);
    }
    function test_keyboard_navigation_and_save_release() {
        preview(); press(Qt.Key_Tab, 23); compare(capture.selection, 0);
        release(Qt.Key_Tab, 23); compare(capture.selection, 1);
        press(Qt.Key_Shift, 50, Qt.ShiftModifier);
        press(Qt.Key_Backtab, 23, Qt.ShiftModifier); release(Qt.Key_Backtab, 23, Qt.ShiftModifier);
        compare(capture.selection, 1); release(Qt.Key_Shift, 50, Qt.ShiftModifier); compare(capture.selection, 0);
        press(Qt.Key_Return, 36); compare(saves.count, 0);
        release(Qt.Key_Return, 36); compare(saves.count, 1);
    }
    function test_keyboard_retry_and_cancel() {
        preview(); press(Qt.Key_Tab, 23); release(Qt.Key_Tab, 23);
        press(Qt.Key_Space, 65); release(Qt.Key_Space, 65); compare(capture.phase, "recording");
        preview();
        for (let i = 0; i < 2; i++) { press(Qt.Key_Tab, 23); release(Qt.Key_Tab, 23); }
        press(Qt.Key_Return, 36); release(Qt.Key_Return, 36); verify(!capture.editing);
    }
}
