import QtQuick

// Capture state is independent of the window and resolver process. Every key,
// including keys pressed in the preview, participates in release gating.
QtObject {
    id: root
    property bool editing: false
    property bool protectedInput: false
    property string phase: "waiting"
    property var held: ({})
    property int modifiers: 0
    property int chordModifiers: 0
    property string candidate: ""
    property string error: ""
    property bool pendingClose: false
    property bool saving: false
    property int generation: 0
    property var keyboardAction: null
    property int selection: 0
    readonly property int chordMask: Qt.MetaModifier | Qt.ControlModifier | Qt.AltModifier | Qt.ShiftModifier
    readonly property bool keysDown: Object.keys(held).length > 0 || modifiers !== 0
    readonly property bool canSave: editing && protectedInput && phase === "preview" && !keysDown && !saving && !pendingClose
    signal resolveRequested(int code, int key, int generation)
    signal saveRequested(string shortcut)
    signal finished()

    function modifier(key) {
        if (key === Qt.Key_Meta || key === Qt.Key_Super_L || key === Qt.Key_Super_R) return Qt.MetaModifier;
        if (key === Qt.Key_Control) return Qt.ControlModifier;
        if (key === Qt.Key_Alt) return Qt.AltModifier;
        if (key === Qt.Key_Shift) return Qt.ShiftModifier;
        return 0;
    }
    function heldModifiers() {
        return Object.values(held).reduce((mask, key) => mask | modifier(key), 0);
    }
    function reset() {
        generation++;
        held = ({}); modifiers = 0; candidate = ""; error = "";
        phase = protectedInput ? "recording" : "waiting";
        keyboardAction = null; selection = 0;
    }
    function begin() {
        if (editing) return;
        pendingClose = false; saving = false;
        reset(); editing = true;
    }
    function finish() {
        editing = false; pendingClose = false; saving = false;
        reset(); finished();
    }
    function cancel() {
        if (!editing || saving) return;
        if (protectedInput && keysDown) { pendingClose = true; keyboardAction = null; }
        else finish();
    }
    function retry() {
        if (editing && protectedInput && !keysDown && !saving && !pendingClose) reset();
    }
    function commit() {
        if (!canSave) return;
        saving = true; saveRequested(candidate);
    }
    function saved(ok, message) {
        if (!editing || !saving) return;
        saving = false;
        if (ok) cancel();
        else error = message || "Could not save the shortcut. Try again.";
    }
    onProtectedInputChanged: {
        if (!editing) return;
        keyboardAction = null;
        if (!protectedInput) {
            if (pendingClose) { finish(); return; }
            if (keysDown || phase !== "preview") {
                const interrupted = keysDown;
                reset();
                if (interrupted) error = "Focus changed while keys were held. Release them, then record again.";
            }
        } else if (phase === "waiting") phase = "recording";
    }
    function press(event) {
        if (!editing) return;
        event.accepted = true;
        if (event.isAutoRepeat) return;
        if (!protectedInput) { if (event.key === Qt.Key_Escape) cancel(); return; }
        const code = event.nativeScanCode || event.key;
        if (held[code] !== undefined) return;
        const wasClear = !keysDown;
        const onlyShift = (modifiers & ~Qt.ShiftModifier) === 0;
        held = Object.assign({}, held, {[code]: event.key});
        modifiers = (event.modifiers & chordMask) | heldModifiers();
        if (saving || pendingClose) return;
        if (event.key === Qt.Key_Escape) { cancel(); return; }
        if (phase === "preview") {
            const tab = event.key === Qt.Key_Tab || event.key === Qt.Key_Backtab;
            if (tab && (wasClear || onlyShift))
                keyboardAction = {code: code, action: "tab", backward: !!(modifiers & Qt.ShiftModifier)};
            else if (wasClear && modifiers === 0 && [Qt.Key_Return, Qt.Key_Enter, Qt.Key_Space].includes(event.key))
                keyboardAction = {code: code, action: "activate"};
            else keyboardAction = null;
            return;
        }
        if (phase !== "recording") return;
        if (modifier(event.key) || event.key === Qt.Key_AltGr) {
            error = "Add another key, such as F10. Modifier-only shortcuts are not supported.";
            return;
        }
        if (event.modifiers & (Qt.GroupSwitchModifier | Qt.KeypadModifier)) {
            error = "Use a key outside the numeric keypad, without AltGr.";
            return;
        }
        chordModifiers = modifiers;
        phase = "resolving";
        resolveRequested(event.nativeScanCode, event.key, generation);
    }
    function resolved(token, key, message) {
        if (token !== generation || !editing || !protectedInput || phase !== "resolving") return;
        if (message || !/^[A-Z0-9_]+$/.test(key) || /^(ISO_|META_|HYPER_)/.test(key)) {
            error = message || "This key is not supported. Try another key.";
            phase = "recording";
            return;
        }
        if (!chordModifiers && !/^(F([1-9]|[12][0-9]|3[0-5])|PAUSE|INSERT)$/.test(key)) {
            error = "Add Super, Control, Alt or Shift, or use a function key.";
            phase = "recording";
            return;
        }
        const parts = [[Qt.MetaModifier, "SUPER"], [Qt.ControlModifier, "CTRL"],
            [Qt.AltModifier, "ALT"], [Qt.ShiftModifier, "SHIFT"]]
            .filter(pair => chordModifiers & pair[0]).map(pair => pair[1]);
        candidate = parts.concat([key]).join(" + ");
        phase = "preview"; error = "";
    }
    function release(event) {
        if (!editing) return;
        event.accepted = true;
        if (!protectedInput || event.isAutoRepeat) return;
        const code = event.nativeScanCode || event.key;
        const key = held[code] === undefined ? event.key : held[code];
        const next = Object.assign({}, held); delete next[code]; held = next;
        modifiers = ((event.modifiers & chordMask) & ~modifier(key)) | heldModifiers();
        if (keyboardAction && keyboardAction.code === code) keyboardAction.released = true;
        if (keysDown) return;
        if (pendingClose) { finish(); return; }
        if (keyboardAction && keyboardAction.released) {
            const action = keyboardAction; keyboardAction = null;
            if (action.action === "tab") selection = (selection + (action.backward ? 2 : 1)) % 3;
            else if (selection === 0) commit();
            else if (selection === 1) retry();
            else cancel();
        }
    }
}
