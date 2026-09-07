// Read the compositor's keymap without creating or focusing a window. This
// classic GJS script is also embedded in the CLI and run with gjs -c, so lookup
// never depends on the shell's virtual URLs or the installation's symlinks.
imports.gi.versions.Gtk = '4.0';
imports.gi.versions.Gdk = '4.0';
const Gtk = imports.gi.Gtk;
const Gdk = imports.gi.Gdk;
const System = imports.system;

const special = new Map([
    ['Escape', 0x01000000], ['Tab', 0x01000001], ['ISO_Left_Tab', 0x01000002],
    ['BackSpace', 0x01000003], ['Return', 0x01000004], ['KP_Enter', 0x01000005],
    ['Insert', 0x01000006], ['Delete', 0x01000007], ['Pause', 0x01000008],
    ['Print', 0x01000009], ['Home', 0x01000010], ['End', 0x01000011],
    ['Left', 0x01000012], ['Up', 0x01000013], ['Right', 0x01000014],
    ['Down', 0x01000015], ['Page_Up', 0x01000016], ['Page_Down', 0x01000017],
]);
for (let n = 1; n <= 35; n++) special.set(`F${n}`, 0x01000030 + n - 1);

function qtKey(keyval) {
    const name = Gdk.keyval_name(keyval);
    if (special.has(name)) return special.get(name);
    const unicode = Gdk.keyval_to_unicode(keyval);
    if (!unicode) return 0;
    const upper = String.fromCodePoint(unicode).toUpperCase();
    return [...upper].length === 1 ? upper.codePointAt(0) : 0;
}

function resolveKey(display, code, key) {
    if (!Number.isInteger(code) || code < 8 || code > 65535 || !key)
        throw new Error('This key has no usable hardware code. Try another key.');
    // Function/navigation keys have layout-independent names. A virtual or
    // secondary keyboard can have a different keymap from a newly connected
    // GDK client; these Qt key values already identify the correct symbol.
    for (const [name, value] of special) {
        if (value === key) return name === 'ISO_Left_Tab' ? 'TAB' : name.toUpperCase();
    }
    const [ok, entries, values] = display.map_keycode(code);
    const bases = new Set();
    if (ok) entries.forEach((entry, i) => {
        if (qtKey(values[i]) !== key) return;
        const [translated, base] = display.translate_key(code, 0, entry.group);
        const name = translated ? Gdk.keyval_name(base) : '';
        if (name && /^[a-zA-Z0-9_]+$/.test(name)) bases.add(name.toUpperCase());
    });
    // QML does not expose the layout group. Never guess when the same Qt key
    // belongs to different base keys across the available layouts.
    if (bases.size !== 1)
        throw new Error('This key could not be identified in your keyboard layout. Try another key.');
    return [...bases][0];
}

if (ARGV.length === 2) {
    try {
        Gtk.init();
        print(JSON.stringify({key: resolveKey(Gdk.Display.get_default(), Number(ARGV[0]), Number(ARGV[1]))}));
    } catch (error) {
        print(JSON.stringify({error: error.message}));
        System.exit(1);
    }
}
