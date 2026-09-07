import Gtk from 'gi://Gtk?version=4.0';
import Gdk from 'gi://Gdk?version=4.0';
import GLib from 'gi://GLib';

const modifiers = new Map([
    ['Super_L', 'SUPER'], ['Super_R', 'SUPER'],
    ['Control_L', 'CTRL'], ['Control_R', 'CTRL'],
    ['Alt_L', 'ALT'], ['Alt_R', 'ALT'],
    ['Shift_L', 'SHIFT'], ['Shift_R', 'SHIFT'],
]);
const masks = [['SUPER', Gdk.ModifierType.SUPER_MASK], ['CTRL', Gdk.ModifierType.CONTROL_MASK],
    ['ALT', Gdk.ModifierType.ALT_MASK], ['SHIFT', Gdk.ModifierType.SHIFT_MASK]];
const chordMask = masks.reduce((all, [, mask]) => all | mask, 0);
const modifierMask = name => masks.find(([modifier]) => modifier === modifiers.get(name))?.[1] || 0;

// Hyprland's normal bindings use the base keysym, before Shift or Caps Lock.
// Use the event's hardware keycode and active layout, never an English-keyboard
// punctuation table: Ctrl+Shift+1 must remain CTRL + SHIFT + 1 on a US layout.
export function baseKeyName(display, keyval, keycode, group) {
    const [translated, base] = display.translate_key(keycode, 0, group);
    if (!translated) throw new Error('This key could not be identified. Try another key.');
    const name = Gdk.keyval_name(base);
    if (!name || !/^[a-zA-Z0-9_]+$/.test(name)) throw new Error('This key is not supported. Try another key.');
    return name;
}

// Independent of GTK focus and compositor requests so key sequences can be
// tested without injecting global keys, recording audio, or changing bindings.
export class ShortcutCapture {
    constructor() { this.reset(); }
    reset() {
        this.phase = 'waiting';
        this.held = new Map();
        this.candidate = '';
        this.error = '';
        this.activeModifiers = 0;
    }
    arm() { if (this.phase === 'waiting') this.phase = 'recording'; }
    close() { this.phase = 'closed'; this.held.clear(); this.activeModifiers = 0; }
    get keysDown() { return this.held.size > 0 || this.activeModifiers !== 0; }
    get canSave() { return this.phase === 'preview' && !this.keysDown; }
    heldModifierMask() { return [...this.held.values()].reduce((state, name) => state | modifierMask(name), 0); }
    observeModifiers(state) { this.activeModifiers = (state & chordMask) | this.heldModifierMask(); }
    hold(name, code, state) { this.held.set(code, name); this.observeModifiers(state); }
    press(name, code, state, baseName = name) {
        if (name === 'Escape') return 'cancel';
        if (!['recording', 'preview'].includes(this.phase) || this.held.has(code)) return null;
        this.hold(name, code, state);
        if (this.phase === 'preview') return null;
        if (modifiers.has(name)) {
            this.error = 'Modifier-only shortcuts cannot reliably stop recording. Add a key such as F10.';
            return null;
        }
        if (/^(ISO_|Meta_|Hyper_)/.test(name)) {
            this.error = 'Use a function key or a combination such as Super + F10.';
            return null;
        }
        const active = new Set(masks.filter(([, mask]) => (state & mask) !== 0).map(([name]) => name));
        for (const heldName of this.held.values()) if (modifiers.has(heldName)) active.add(modifiers.get(heldName));
        const key = baseName.toUpperCase();
        if (!/^[A-Z0-9_]+$/.test(key)) {
            this.error = 'This key is not supported. Try another key.';
            return null;
        }
        if (active.size === 0 && !/^(F([1-9]|[12][0-9]|3[0-5])|PAUSE|INSERT)$/.test(key)) {
            this.error = 'Add a modifier such as Super or Control, or use a function key.';
            return null;
        }
        this.candidate = [...masks.map(([name]) => name).filter(name => active.has(name)), key].join(' + ');
        this.phase = 'preview';
        this.error = '';
        return 'preview';
    }
    release(releasedName, code, state = this.activeModifiers) {
        const name = this.held.get(code) || releasedName;
        this.held.delete(code);
        // GDK's release event may still contain the modifier being released.
        // Keep the bit only if another known key of the same kind stays down.
        this.activeModifiers = ((state & chordMask) & ~modifierMask(name)) | this.heldModifierMask();
        return null;
    }
}

export class ShortcutRecorder {
    constructor({parent, current, save, closed, timeout = 30000}) {
        this.capture = new ShortcutCapture();
        this.save = save;
        this.onClosed = closed;
        this.timeoutMs = timeout;
        this.surface = null;
        this.inhibitSignal = 0;
        this.inhibitRequested = false;
        this.hadFocus = false;
        this.closed = false;
        this.saving = false;
        this.timer = 0;
        this.grantTimer = 0;
        this.keyboardAction = null;
        this.pendingClose = null;
        this.window = new Gtk.Window({application: parent.application, transient_for: parent,
            modal: true, destroy_with_parent: true, resizable: false,
            title: 'Record shortcut · JustSpeak', default_width: 440});
        const content = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 16,
            margin_top: 24, margin_bottom: 24, margin_start: 24, margin_end: 24});
        const makeLabel = (text, options = {}) => new Gtk.Label({label: text, use_markup: false,
            wrap: true, xalign: 0, max_width_chars: 48, ...options});
        const heading = makeLabel('Record a hold-to-talk shortcut');
        heading.add_css_class('title-2');
        content.append(heading);
        content.append(makeLabel(`Current shortcut: ${current}`));
        this.message = makeLabel('Waiting for the desktop to suspend its shortcuts. Do not press your shortcut yet.');
        content.append(this.message);
        this.preview = makeLabel('Waiting…', {xalign: 0.5});
        this.preview.add_css_class('title-1');
        content.append(this.preview);
        this.error = makeLabel('', {visible: false});
        this.error.add_css_class('error');
        content.append(this.error);
        content.append(makeLabel('Escape cancels. Your existing shortcut changes only after you click Save.', {opacity: 0.7}));
        const actions = new Gtk.Box({orientation: Gtk.Orientation.HORIZONTAL, spacing: 10, halign: Gtk.Align.END});
        this.cancelButton = new Gtk.Button({label: 'Cancel'});
        this.cancelButton.connect('clicked', () => { if (!this.saving) this.close(); });
        this.again = new Gtk.Button({label: 'Record again', visible: false});
        this.again.connect('clicked', () => this.restart());
        this.saveButton = new Gtk.Button({label: 'Save', sensitive: false});
        this.saveButton.add_css_class('suggested-action');
        this.saveButton.connect('clicked', () => { void this.commit(); });
        actions.append(this.cancelButton);
        actions.append(this.again);
        actions.append(this.saveButton);
        content.append(actions);
        this.window.set_child(content);
        this.controller = new Gtk.EventControllerKey({propagation_phase: Gtk.PropagationPhase.CAPTURE});
        this.controller.connect('key-pressed', (_controller, keyval, keycode, state) => this.keyPressed(keyval, keycode, state));
        this.controller.connect('key-released', (_controller, keyval, keycode, state) => this.keyReleased(keyval, keycode, state));
        this.controller.connect('modifiers', (_controller, state) => {
            if (this.closed) return false;
            this.capture.observeModifiers(state);
            this.afterRelease();
            return false;
        });
        this.window.add_controller(this.controller);
        this.window.connect('map', () => this.requestInhibition());
        this.window.connect('notify::is-active', () => this.focusChanged(this.window.is_active));
        this.window.connect('close-request', () => { if (!this.saving) this.close(); return true; });
        this.window.connect('unmap', () => this.close(this.saving ? 'The shortcut save is still finishing.' : '', true));
    }

    present() { if (!this.closed) this.window.present(); }
    clearTimer(name) {
        if (this[name]) GLib.source_remove(this[name]);
        this[name] = 0;
    }
    requestInhibition() {
        if (this.closed || this.inhibitRequested) return;
        try {
            this.surface = this.window.get_surface();
            this.inhibitSignal = this.surface.connect('notify::shortcuts-inhibited', () => this.inhibitionChanged());
            this.inhibitRequested = true;
            this.surface.inhibit_system_shortcuts(null);
            this.grantTimer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 4000, () => {
                this.grantTimer = 0;
                this.close('Shortcut recording was not enabled by the desktop. Keep JustSpeak focused and try again.');
                return GLib.SOURCE_REMOVE;
            });
            this.inhibitionChanged();
        } catch (error) {
            this.close(`Shortcut recording is unavailable: ${error.message}`);
        }
    }
    inhibitionChanged() {
        if (this.closed) return;
        if (this.surface?.shortcuts_inhibited === true && this.window.is_active) {
            this.clearTimer('grantTimer');
            if (this.capture.phase === 'waiting') {
                this.capture.arm();
                this.capture.observeModifiers(this.window.get_display().get_default_seat()?.get_keyboard()?.get_modifier_state() || 0);
                this.startTimeout();
                this.render();
            }
        } else if (this.capture.phase !== 'waiting') {
            this.close(this.saving ? 'The shortcut save is still finishing.'
                : 'Shortcut recording canceled because the desktop restored its shortcuts.', true);
        }
    }
    focusChanged(active) {
        if (this.closed) return;
        if (active) { this.hadFocus = true; this.inhibitionChanged(); }
        else if (this.hadFocus) this.close(this.saving ? 'The shortcut save is still finishing.'
            : 'Shortcut recording canceled when the window lost focus.', true);
    }
    startTimeout() {
        this.clearTimer('timer');
        this.timer = GLib.timeout_add(GLib.PRIORITY_DEFAULT, this.timeoutMs, () => {
            this.timer = 0;
            this.close('Shortcut recording timed out. Your previous shortcut is unchanged.');
            return GLib.SOURCE_REMOVE;
        });
    }
    keyPressed(keyval, keycode, state) {
        const name = Gdk.keyval_name(keyval);
        if (this.closed) return true;
        if (this.saving || this.pendingClose) {
            this.capture.hold(name, keycode, state);
            this.render();
            return true;
        }
        if (name === 'Escape') {
            if (this.surface?.shortcuts_inhibited && this.window.is_active) this.capture.hold(name, keycode, state);
            this.close();
            return true;
        }
        // Keep inhibition through the preview and *all* key releases. Otherwise
        // releasing F9/F10 could run another app's release-only binding.
        if (this.capture.phase === 'waiting') { this.capture.hold(name, keycode, state); return true; }
        if (this.capture.phase === 'preview') {
            // Also track keys pressed after capture. A mouse click on Save must
            // never restore global shortcuts while any of these keys is held.
            const tab = name === 'Tab' || name === 'ISO_Left_Tab';
            const onlyShift = [...this.capture.held.values()].every(key => modifierMask(key) === Gdk.ModifierType.SHIFT_MASK)
                && (this.capture.activeModifiers & ~Gdk.ModifierType.SHIFT_MASK) === 0;
            if (!this.capture.keysDown || (tab && onlyShift)) {
                if (name === 'Return' && (state & chordMask) === 0) this.keyboardAction = {code: keycode, action: 'save'};
                else if (tab)
                    this.keyboardAction = {code: keycode, action: state & Gdk.ModifierType.SHIFT_MASK ? 'backward' : 'forward'};
                else if (name === 'space' && (state & chordMask) === 0)
                    this.keyboardAction = {code: keycode, action: 'button', button: this.window.get_focus()};
            } else if (!this.capture.held.has(keycode)) this.keyboardAction = null;
            this.capture.press(name, keycode, state);
            this.render();
            return true;
        }
        if (this.surface?.shortcuts_inhibited !== true || !this.window.is_active) {
            this.close('Shortcut recording lost keyboard protection. Try again.', true);
            return true;
        }
        try {
            const base = modifiers.has(name) ? name : baseKeyName(this.window.get_display(), keyval, keycode, this.controller.get_group());
            this.capture.press(name, keycode, state, base);
        } catch (error) {
            this.capture.hold(name, keycode, state);
            this.capture.error = error.message;
        }
        this.render();
        return true;
    }
    keyReleased(keyval, keycode, state) {
        if (this.closed) return;
        this.capture.release(Gdk.keyval_name(keyval), keycode, state);
        if (this.keyboardAction?.code === keycode) this.keyboardAction.released = true;
        this.afterRelease();
    }
    afterRelease() {
        if (this.pendingClose && !this.capture.keysDown) { this.close(this.pendingClose.message); return; }
        this.render();
        if (this.keyboardAction?.released && !this.capture.keysDown) {
            const action = this.keyboardAction;
            this.keyboardAction = null;
            if (!this.capture.canSave || this.saving) return;
            if (action.action === 'save') void this.commit();
            else if (action.action === 'forward' || action.action === 'backward')
                this.window.child_focus(action.action === 'backward' ? Gtk.DirectionType.TAB_BACKWARD : Gtk.DirectionType.TAB_FORWARD);
            else if ([this.saveButton, this.again, this.cancelButton].includes(action.button)) action.button.emit('clicked');
        }
    }
    render() {
        if (this.closed) return;
        const preview = this.capture.phase === 'preview';
        this.preview.label = preview ? this.capture.candidate : 'Press your shortcut';
        this.message.label = this.pendingClose ? 'Release all keys to close the recorder, or switch to another window to cancel. Desktop shortcuts remain suspended while you release the keys here.'
            : this.saving ? 'Saving shortcut…'
            : preview ? (this.capture.keysDown ? 'Release all keys to continue.' : 'Shortcut captured. Click Save or press Enter to use it, or record again.')
                : 'Press a function key or a modifier with another key, such as Super + F10. Desktop shortcuts are suspended while this window stays focused.';
        this.error.label = this.capture.error;
        this.error.visible = Boolean(this.capture.error);
        this.saveButton.sensitive = this.capture.canSave && !this.saving && !this.pendingClose;
        this.cancelButton.sensitive = !this.saving && !this.pendingClose;
        this.again.visible = preview;
        this.again.sensitive = !this.capture.keysDown && !this.saving && !this.pendingClose;
    }
    restart() {
        if (this.closed || this.saving || this.pendingClose || this.capture.keysDown) return;
        this.capture.reset();
        this.keyboardAction = null;
        this.inhibitionChanged();
    }
    async commit() {
        if (this.closed || this.saving || this.pendingClose || !this.capture.canSave) return;
        this.saving = true;
        this.window.deletable = false;
        this.clearTimer('timer');
        this.render();
        try {
            await this.save(this.capture.candidate);
            this.close();
        } catch (error) {
            if (!this.closed) {
                this.capture.error = String(error.message || error);
                this.startTimeout();
            }
        } finally {
            this.saving = false;
            if (!this.closed) this.window.deletable = true;
            this.render();
        }
    }
    close(message = '', force = false) {
        if (this.closed) return;
        if (!force && this.inhibitRequested && this.surface?.shortcuts_inhibited && this.window.is_active && this.capture.keysDown) {
            if (!this.pendingClose) {
                this.pendingClose = {message};
                this.keyboardAction = null;
                this.clearTimer('timer');
            }
            this.render();
            return;
        }
        this.closed = true;
        this.clearTimer('timer');
        this.clearTimer('grantTimer');
        this.capture.close();
        if (this.surface) {
            if (this.inhibitSignal) this.surface.disconnect(this.inhibitSignal);
            this.inhibitSignal = 0;
            if (this.inhibitRequested) this.surface.restore_system_shortcuts();
        }
        this.inhibitRequested = false;
        // Gtk 4.6 can retain a destroyed child until GJS GC. Detach its transient
        // parent now so later disposal cannot disconnect signals from a parent
        // that has already been finalized.
        this.window.set_transient_for(null);
        this.window.destroy();
        this.onClosed?.(message);
    }
}
