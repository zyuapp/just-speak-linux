import Gtk from 'gi://Gtk?version=4.0';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Pango from 'gi://Pango';
import System from 'system';
import {Backend, sampleMenu, delay} from './backend.js';

const visibleSmoke = ARGV.includes('--smoke-test-visible');
const smoke = visibleSmoke || ARGV.includes('--smoke-test');
const app = new Gtk.Application({application_id: smoke ? 'io.github.zyuapp.JustSpeak.Smoke' : 'io.github.zyuapp.JustSpeak',
    // FLAGS_NONE works with Ubuntu 22.04's GLib 2.72; DEFAULT_FLAGS needs 2.74.
    flags: smoke ? Gio.ApplicationFlags.NON_UNIQUE : Gio.ApplicationFlags.FLAGS_NONE});
const backend = new Backend(smoke);
let window;
let smokeExit = 0;
let interval = 0;
let polling = false;
let busy = false;
let rendering = false;
let checking = false;
let installing = false;
let lastMenu = 0;
let lastUpdate = 0;
let metadata = {settings: {}, inputs: [], history: [], desktop: {}};
let status = {phase: 'disconnected', model_ready: false, can_cancel: false, shortcut: 'F10'};
let update = null;
let historySignature = '';
let inputSignature = '';
const controls = {};
const switches = new Map();

function label(text, options = {}) {
    return new Gtk.Label({label: text, xalign: 0, use_markup: false, ...options});
}
function button(text, callback, options = {}) {
    const result = new Gtk.Button({label: text, ...options});
    result.connect('clicked', () => Promise.resolve(callback()).catch(showError));
    return result;
}
function row(...children) {
    const result = new Gtk.Box({orientation: Gtk.Orientation.HORIZONTAL, spacing: 10});
    children.forEach(child => result.append(child));
    return result;
}
function section(text, parent) {
    const heading = label(text);
    heading.add_css_class('heading');
    heading.set_margin_top(10);
    parent.append(heading);
}
function showError(error) {
    controls.message.label = String(error.message || error).slice(0, 1200);
    controls.message.add_css_class('error');
    controls.message.visible = true;
}
function showNotice(text) {
    controls.message.remove_css_class('error');
    controls.message.label = text;
    controls.message.visible = Boolean(text);
}
function recording() { return ['recording', 'transcribing', 'updating'].includes(status.phase); }
function canEdit() { return !busy && !installing && !recording() && status.phase !== 'disconnected' && Boolean(metadata.version); }
function capabilities() { return metadata.desktop || {}; }

function renderStatus() {
    let text;
    switch (status.phase) {
    case 'recording': text = `Listening · ${Math.floor(status.elapsed_seconds || 0)}s`; break;
    case 'transcribing': text = 'Transcribing locally…'; break;
    case 'loading': text = 'Loading speech model…'; break;
    case 'updating': text = status.message || 'Updating JustSpeak…'; break;
    case 'error': text = status.message || 'JustSpeak needs attention'; break;
    case 'disconnected': text = 'JustSpeak is stopped'; break;
    default: text = status.model_ready ? 'Ready to dictate' : 'Speech model not ready';
    }
    controls.status.label = text;
    controls.start.label = status.phase === 'disconnected' ? 'Start JustSpeak'
        : status.phase === 'recording' ? 'Finish dictation' : 'Start dictation';
    controls.start.tooltip_text = capabilities().automatic_paste
        ? `Hides this window so your application receives dictation. Press and release ${status.shortcut || 'F10'} to finish, or reopen JustSpeak.`
        : 'Record here, then use Finish and paste the result from your clipboard.';
    controls.start.sensitive = !busy && !installing && (['disconnected', 'recording'].includes(status.phase)
        || (status.model_ready && ['idle', 'error'].includes(status.phase)));
    controls.cancel.visible = status.can_cancel === true;
    controls.cancel.sensitive = !busy;
    controls.input.sensitive = canEdit();
    controls.shortcut.sensitive = canEdit() && capabilities().shortcut_editing === true;
    controls.apply.sensitive = controls.shortcut.sensitive;
    controls.clear.sensitive = canEdit() && metadata.history.length > 0;
    controls.history.sensitive = canEdit();
    controls.restart.sensitive = !busy && !installing && !recording();
    controls.quit.sensitive = !busy && !installing && status.phase !== 'disconnected';
    controls.install.sensitive = !busy && !installing && !recording() && !checking;
    for (const [key, control] of switches) control.sensitive = canEdit()
        && (key !== 'paste' || capabilities().automatic_paste === true);
}

function renderMenu() {
    rendering = true;
    try {
        controls.version.label = `JustSpeak ${metadata.version || '0.2.0'}`;
        const desktop = capabilities();
        controls.desktop.label = desktop.shortcut_editing
            ? `Hold ${metadata.settings.shortcut || status.shortcut} in any application to dictate.`
            : 'Use Start and Finish here, then paste from the clipboard. Global shortcuts and automatic paste are not available on this desktop yet.';
        controls.experimental.visible = desktop.experimental === true;
        if (!controls.shortcut.has_focus) controls.shortcut.text = metadata.settings.shortcut || status.shortcut || 'F10';
        for (const [key, control] of switches) control.active = metadata.settings[key] === true;
        const signature = JSON.stringify([metadata.inputs, metadata.settings.input]);
        if (signature !== inputSignature) {
            inputSignature = signature;
            const options = [{id: 'default', name: 'System default microphone'}, ...(metadata.inputs || [])];
            const selected = metadata.settings.input === null || metadata.settings.input === undefined ? 'default' : String(metadata.settings.input);
            if (!options.some(option => String(option.id) === selected)) options.push({id: selected, name: `Unavailable input · ${selected}`});
            controls.inputIds = options.map(option => String(option.id));
            controls.input.model = Gtk.StringList.new(options.map(option => option.name + (option.is_default ? ' · default' : '')));
            controls.input.selected = controls.inputIds.indexOf(selected);
        }
        controls.inputError.label = metadata.input_error || '';
        controls.inputError.visible = Boolean(metadata.input_error);
        const historyKey = JSON.stringify([metadata.history, desktop.automatic_paste]);
        if (historyKey !== historySignature) {
            historySignature = historyKey;
            while (controls.history.get_first_child()) controls.history.remove(controls.history.get_first_child());
            for (const entry of metadata.history.slice(0, 10)) {
                const summary = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 5});
                summary.append(label(String(entry.text), {wrap: true, wrap_mode: Pango.WrapMode.WORD_CHAR,
                    lines: 3, ellipsize: Pango.EllipsizeMode.END, max_width_chars: 46}));
                const time = label(new Date(Number(entry.created_at) * 1000).toLocaleString(), {opacity: 0.6});
                time.add_css_class('caption');
                summary.append(time);
                const paste = button('', async () => {
                    if (desktop.automatic_paste) await outsideAction(['history', 'paste', String(entry.id)]);
                    else await mutate(['history', 'copy', String(entry.id)], 'Copied to clipboard');
                }, {hexpand: true, tooltip_text: desktop.automatic_paste ? 'Paste this transcript' : 'Copy this transcript'});
                paste.set_child(summary);
                const copy = button('Copy', () => mutate(['history', 'copy', String(entry.id)], 'Copied to clipboard'), {valign: Gtk.Align.CENTER});
                controls.history.append(row(paste, copy));
            }
        }
        controls.empty.visible = metadata.history.length === 0;
        controls.empty.label = metadata.settings.history_enabled === false
            ? 'History is off. New transcripts are not saved.' : 'Your last 10 transcripts will appear here.';
    } finally {
        rendering = false;
    }
    renderStatus();
}

async function refreshMenu() {
    const data = await backend.call(['menu', '--json'], {json: true});
    if (!data.settings || !Array.isArray(data.inputs) || !Array.isArray(data.history)) throw new Error('Invalid settings response from JustSpeak.');
    data.history = data.history.slice(0, 10);
    metadata = data;
    lastMenu = Date.now();
    renderMenu();
}
async function poll() {
    if (polling || busy) return;
    polling = true;
    const previous = status.phase;
    try {
        try {
            status = await backend.call(['status', '--json'], {json: true, timeout: 4000});
        } catch (error) {
            status = {phase: 'disconnected', model_ready: false, can_cancel: false, shortcut: status.shortcut};
        }
        renderStatus();
        if (status.phase !== 'disconnected' && (Date.now() - lastMenu > 8000 || previous !== status.phase)) {
            try { await refreshMenu(); } catch (error) { showError(error); }
        }
        if (!smoke && status.phase === 'idle' && metadata.settings.auto_check_updates === true
            && Date.now() - lastUpdate > 6 * 60 * 60 * 1000) void checkUpdates(false);
    } finally {
        polling = false;
    }
}
async function mutate(args, message = '') {
    if (busy || installing) return;
    busy = true;
    showNotice(args[0] === 'shortcut' ? 'Saving shortcut…' : '');
    renderStatus();
    try {
        await backend.call(args, {timeout: args[0] === 'shortcut' ? 25000 : 8000});
        showNotice(message);
        if (!['quit', 'launch', 'restart'].includes(args[0])) await refreshMenu();
        else lastMenu = 0;
    } finally {
        busy = false;
        await poll();
        renderStatus();
    }
}
async function outsideAction(args) {
    window.set_visible(false);
    await delay(200);
    try { await mutate(args); }
    catch (error) { window.present(); throw error; }
}
async function checkUpdates(manual) {
    if (checking || installing || (!manual && Date.now() - lastUpdate < 6 * 60 * 60 * 1000)) return;
    checking = true;
    lastUpdate = Date.now();
    controls.check.sensitive = false;
    controls.update.label = 'Checking for updates…';
    try {
        update = await backend.call(['update', 'check', '--json'], {json: true, timeout: 45000});
        controls.update.label = update.available ? `JustSpeak ${update.latest_version} is available.`
            : update.latest_version ? 'JustSpeak is up to date.' : 'No published release yet.';
        controls.install.visible = update.available === true;
    } catch (error) {
        controls.update.label = `Could not check for updates: ${error.message}`;
    } finally {
        checking = false;
        controls.check.sensitive = true;
        renderStatus();
    }
}
async function installUpdate() {
    if (!update?.available || busy || installing || recording()) return;
    installing = true;
    controls.update.label = 'Installing update… JustSpeak will restart when it is ready.';
    controls.check.sensitive = false;
    renderStatus();
    try {
        // The CLI applies from a transient service that survives UI restarts.
        await backend.call(['update', 'install'], {timeout: 0});
        controls.update.label = 'Update installed. Close and reopen this window to load the new interface.';
        controls.install.visible = false;
        // The restarted service may not have created its socket yet. A menu
        // refresh failure here must not turn a successful install into failure.
        lastMenu = 0;
    } catch (error) {
        controls.update.label = `Update failed: ${error.message}`;
    } finally {
        installing = false;
        controls.check.sensitive = true;
        renderStatus();
    }
}

function buildWindow(application) {
    window = new Gtk.ApplicationWindow({application, title: 'JustSpeak', default_width: 520, default_height: 760});
    const header = new Gtk.HeaderBar();
    header.set_title_widget(label('JustSpeak'));
    header.pack_end(button('Refresh', async () => { await poll(); await refreshMenu(); }));
    window.set_titlebar(header);
    const scroll = new Gtk.ScrolledWindow({hscrollbar_policy: Gtk.PolicyType.NEVER, vexpand: true});
    const content = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 12,
        halign: Gtk.Align.CENTER, width_request: 480,
        margin_top: 20, margin_bottom: 20, margin_start: 20, margin_end: 20});
    scroll.set_child(content);
    window.set_child(scroll);
    controls.status = label('Connecting…', {wrap: true});
    controls.status.add_css_class('title-2');
    content.append(controls.status);
    controls.desktop = label('', {wrap: true, max_width_chars: 58});
    content.append(controls.desktop);
    controls.experimental = label('This desktop integration is experimental. GNOME behavior has not been verified.', {wrap: true});
    controls.experimental.add_css_class('dim-label');
    content.append(controls.experimental);
    controls.message = label('', {wrap: true, visible: false, selectable: true});
    content.append(controls.message);
    controls.start = button('Start dictation', async () => {
        if (status.phase === 'disconnected') return mutate(['launch'], 'Starting JustSpeak');
        const command = status.phase === 'recording' ? 'stop' : 'start';
        if (capabilities().automatic_paste) return outsideAction([command]);
        return mutate([command], command === 'stop' ? 'Transcribing to clipboard' : 'Listening');
    });
    controls.start.add_css_class('suggested-action');
    controls.cancel = button('Cancel', () => mutate(['cancel'], 'Canceled'), {visible: false});
    content.append(row(controls.start, controls.cancel));
    section('Recent transcripts', content);
    controls.clear = button('Clear history', () => mutate(['history', 'clear'], 'History cleared'), {halign: Gtk.Align.START});
    content.append(controls.clear);
    controls.empty = label('Your last 10 transcripts will appear here.', {wrap: true});
    content.append(controls.empty);
    controls.history = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 8});
    const historyScroll = new Gtk.ScrolledWindow({hscrollbar_policy: Gtk.PolicyType.NEVER,
        min_content_height: 70, max_content_height: 230, propagate_natural_height: true});
    historyScroll.set_child(controls.history);
    content.append(historyScroll);
    section('Microphone', content);
    controls.input = new Gtk.DropDown({hexpand: true});
    controls.inputIds = [];
    controls.input.connect('notify::selected', () => {
        if (!rendering && canEdit() && controls.inputIds[controls.input.selected])
            void mutate(['input', 'set', controls.inputIds[controls.input.selected]], 'Microphone updated').catch(showError);
    });
    content.append(controls.input);
    controls.inputError = label('', {wrap: true, visible: false});
    controls.inputError.add_css_class('error');
    content.append(controls.inputError);
    section('Hold-to-talk shortcut', content);
    controls.shortcut = new Gtk.Entry({placeholder_text: 'e.g. SUPER + F10', hexpand: true, max_length: 96});
    const saveShortcut = () => mutate(['shortcut', 'set', controls.shortcut.text.trim()], 'Shortcut updated');
    controls.shortcut.connect('activate', () => { if (controls.apply.sensitive) void saveShortcut().catch(showError); });
    controls.apply = button('Apply', saveShortcut);
    content.append(row(controls.shortcut, controls.apply));
    section('Preferences', content);
    for (const [key, title] of [['sound_feedback', 'Recording sounds'], ['mute_while_recording', 'Mute other audio while recording'],
        ['paste', 'Paste automatically'], ['history_enabled', 'Save recent transcripts'], ['auto_check_updates', 'Check for updates automatically']]) {
        const toggle = new Gtk.Switch({valign: Gtk.Align.CENTER});
        switches.set(key, toggle);
        toggle.connect('state-set', (_self, value) => {
            if (!rendering && canEdit()) void mutate(['settings', 'set', key, String(value)], 'Settings saved').catch(showError);
            return !rendering;
        });
        content.append(row(label(title, {hexpand: true, wrap: true}), toggle));
    }
    section('Updates', content);
    controls.update = label('Check for new releases of JustSpeak.', {wrap: true});
    content.append(controls.update);
    controls.check = button('Check for updates', () => checkUpdates(true));
    controls.install = button('Install update', installUpdate, {visible: false});
    content.append(row(controls.check, controls.install));
    controls.version = label('JustSpeak 0.2.0', {hexpand: true});
    controls.version.add_css_class('dim-label');
    controls.restart = button('Restart', () => mutate(['restart'], 'Restarting JustSpeak'));
    controls.quit = button('Quit JustSpeak', () => mutate(['quit'], 'JustSpeak stopped'));
    content.append(row(controls.version, controls.restart, controls.quit));
    return window;
}

app.connect('activate', () => {
    if (window) { window.present(); void poll(); return; }
    buildWindow(app);
    if (smoke) {
        try {
        metadata = JSON.parse(JSON.stringify(sampleMenu));
        status = {phase: 'idle', model_ready: true, can_cancel: false, shortcut: 'F10'};
        renderMenu();
        if (!controls.shortcut.sensitive || controls.history.get_first_child() === null) throw new Error('GTK controls did not initialize');
        metadata.desktop = {name: 'wayland-clipboard', automatic_paste: false, shortcut_editing: false, experimental: true};
        renderMenu();
        if (controls.shortcut.sensitive || switches.get('paste').sensitive || !controls.experimental.visible) throw new Error('Desktop capability gating failed');
        status.phase = 'updating';
        renderStatus();
        if (controls.start.sensitive || controls.input.sensitive) throw new Error('Update mutation gate failed');
        print('GTK_SMOKE_COMPLETE: window, plain-text history, settings, desktop capabilities, update gate; no external actions');
        } catch (error) { smokeExit = 1; printerr(error.stack || error.message); }
        if (visibleSmoke && smokeExit === 0) {
            metadata = JSON.parse(JSON.stringify(sampleMenu));
            status = {phase: 'idle', model_ready: true, can_cancel: false, shortcut: 'F10'};
            renderMenu();
            window.present();
            print('GTK_VISUAL_READY: synthetic content only');
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, 8000, () => { app.quit(); return GLib.SOURCE_REMOVE; });
        } else {
            GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => { app.quit(); return GLib.SOURCE_REMOVE; });
        }
        return;
    }
    window.present();
    void poll().catch(showError);
    interval = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1000, () => { void poll().catch(showError); return GLib.SOURCE_CONTINUE; });
});
app.connect('shutdown', () => {
    if (interval) GLib.source_remove(interval);
    backend.close();
});
const exitCode = app.run([]);
System.exit(smoke ? smokeExit : exitCode);
