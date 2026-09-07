import Gdk from 'gi://Gdk?version=4.0';
import GLib from 'gi://GLib';
import System from 'system';
imports.searchPath.unshift(GLib.build_filenamev([GLib.path_get_dirname(System.programPath), '..']));
const {resolveKey, qtKey} = imports['shortcut-keymap'];

function assert(value, message) { if (!value) throw new Error(message); }
function rejected(fn) { try { fn(); } catch (_) { return; } throw new Error('Expected rejection'); }
function fixture(groups) {
    return {
        map_keycode() {
            const keys = [], values = [];
            groups.forEach((names, group) => names.forEach((name, level) => {
                keys.push({group, level}); values.push(Gdk.keyval_from_name(name));
            }));
            return [true, keys, values];
        },
        translate_key(code, state, group) { return [true, Gdk.keyval_from_name(groups[group][0])]; },
    };
}
assert(resolveKey(fixture([['1', 'exclam']]), 10, 33) === '1', 'Shift+1 translated incorrectly');
assert(resolveKey(fixture([['a', 'A']]), 38, 65) === 'A', 'Caps Lock/letter translation');
assert(resolveKey(fixture([['eacute', '2']]), 11, 50) === 'EACUTE', 'Non-US number row translation');
assert(resolveKey(fixture([['semicolon', 'colon']]), 47, 58) === 'SEMICOLON', 'Punctuation translation');
assert(resolveKey(fixture([['F10']]), 76, 0x01000039) === 'F10', 'Function key translation');
assert(resolveKey(fixture([['1']]), 8, 0x01000052) === 'F35', 'Virtual keyboard function key translation');
assert(resolveKey(fixture([['a', 'A'], ['q', 'Q']]), 38, 81) === 'Q', 'Layout group matching');
rejected(() => resolveKey(fixture([['a', 'Q'], ['q', 'Q']]), 38, 81));
rejected(() => resolveKey(fixture([['a']]), 0, 65));
rejected(() => resolveKey(fixture([['a']]), 38, 66));
assert(qtKey(Gdk.keyval_from_name('ISO_Left_Tab')) === 0x01000002, 'Shift+Tab translation');
print('PASS: keymap translation, punctuation, layouts, ambiguous/unknown keys');
