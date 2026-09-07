import Gdk from 'gi://Gdk?version=4.0';
import {ShortcutCapture, baseKeyName} from '../shortcut-recorder.js';

function equal(actual, expected, message) {
    if (actual !== expected) throw new Error(`${message}: expected ${expected}, got ${actual}`);
}
const ctrl = Gdk.ModifierType.CONTROL_MASK;
const shift = Gdk.ModifierType.SHIFT_MASK;
const superMask = Gdk.ModifierType.SUPER_MASK;
let capture = new ShortcutCapture();
capture.press('F10', 76, 0);
equal(capture.candidate, '', 'No capture before compositor confirmation');
capture.arm();
capture.press('F10', 76, 0);
equal(capture.candidate, 'F10', 'Function key');
equal(capture.canSave, false, 'Held function key cannot be saved');
capture.press('F10', 76, 0);
equal(capture.held.size, 1, 'Repeat does not duplicate held keys');
capture.press('F9', 75, 0);
equal(capture.candidate, 'F10', 'Preview is not overwritten by another key');
capture.release('F10', 76);
equal(capture.canSave, false, 'Save also waits for a key pressed during preview');
capture.release('F9', 75);
equal(capture.canSave, true, 'Key release enables explicit save');

capture.reset(); capture.arm();
capture.press('Control_L', 37, 0);
capture.press('Shift_L', 50, ctrl);
const display = {translate_key(code, state, group) {
    equal(code, 10, 'Translate the physical digit key');
    equal(state, 0, 'Remove shift and lock before base translation');
    equal(group, 1, 'Preserve the event keyboard layout');
    return [true, Gdk.KEY_1, 1, 0, 0];
}};
capture.press('exclam', 10, ctrl | shift, baseKeyName(display, Gdk.KEY_exclam, 10, 1));
equal(capture.candidate, 'CTRL + SHIFT + 1', 'Shifted punctuation retains the base keysym');
capture.release('exclam', 10);
capture.release('Control_L', 37);
equal(capture.canSave, false, 'Save waits for the last modifier release');
capture.release('Shift_L', 50);
equal(capture.canSave, true, 'All chord keys released');

capture.reset(); capture.arm();
capture.press('A', 38, ctrl | shift, 'a');
equal(capture.candidate, 'CTRL + SHIFT + A', 'Letter with held modifiers');
capture.reset(); capture.arm();
capture.press('F12', 96, superMask | ctrl);
equal(capture.candidate, 'SUPER + CTRL + F12', 'Canonical modifier order');
capture.release('F12', 96, superMask | ctrl);
equal(capture.canSave, false, 'Modifiers held before the window opened still block Save');
capture.release('Super_L', 133, superMask | ctrl);
equal(capture.canSave, false, 'A second pre-held modifier still blocks Save');
capture.release('Control_R', 105, ctrl);
equal(capture.canSave, true, 'Releasing pre-held modifiers enables Save');

for (const name of ['Super_L', 'Super_R', 'Control_L', 'Control_R', 'Alt_L', 'Alt_R', 'Shift_L', 'Shift_R']) {
    capture.reset(); capture.arm();
    capture.press(name, 133, 0);
    capture.press(name, 133, 0);
    equal(capture.candidate, '', 'Modifier alone does not create a candidate');
    capture.release(name, 133);
    equal(capture.candidate, '', `Standalone ${name} rejected`);
    equal(capture.canSave, false, 'Released standalone modifier cannot be saved');
    equal(Boolean(capture.error), true, 'Modifier rejection explains how to retry');
}
for (const name of ['Alt_L', 'Alt_R']) {
    capture.reset(); capture.arm();
    capture.press(name, 108, 0);
    capture.press('F10', 76, Gdk.ModifierType.ALT_MASK);
    equal(capture.candidate, 'ALT + F10', 'Alt with an ordinary key remains supported');
    equal(capture.error, '', 'Valid chord clears the modifier-only guidance');
    capture.release(name, 108);
    equal(capture.canSave, false, 'Releasing modifier first still waits for the ordinary key');
    capture.release('F10', 76);
    equal(capture.canSave, true, 'Valid Alt chord can be saved after release');
}
capture.reset(); capture.arm();
capture.press('Control_L', 37, 0);
capture.press('Shift_L', 50, ctrl);
capture.release('Control_L', 37);
capture.release('Shift_L', 50);
equal(capture.candidate, '', 'Multiple modifiers alone are not mistaken for a standalone modifier');
capture.press('F10', 76, 0);
capture.release('F10', 76);
equal(capture.candidate, 'F10', 'A new gesture works after invalid input');

capture.reset(); capture.arm();
capture.press('a', 38, 0);
equal(capture.candidate, '', 'Bare typing key is rejected');
equal(Boolean(capture.error), true, 'Unsupported key explains how to retry');
capture.release('a', 38);
capture.press('F9', 75, 0);
equal(capture.candidate, 'F9', 'Conflict validation is deferred until Save');
equal(capture.press('Escape', 9, 0), 'cancel', 'Escape cancels preview');
capture.close();
equal(capture.held.size, 0, 'Cancellation clears held keys');
equal(capture.canSave, false, 'Closed recorder cannot save');
capture.press('F12', 96, 0);
equal(capture.phase, 'closed', 'Closed recorder ignores input');
print('SHORTCUT_CAPTURE_TESTS_COMPLETE: inhibition gate, chords, layout, modifier releases, repeat, preview, cancellation');
