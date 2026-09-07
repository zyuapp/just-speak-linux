import Gtk from 'gi://Gtk?version=4.0';
import Gio from 'gi://Gio';
import GLib from 'gi://GLib';
import Pango from 'gi://Pango';
import System from 'system';
import {Backend, sampleMenu, delay} from './backend.js';
import {ShortcutRecorder} from './shortcut-recorder.js';
import {ModelSetup} from './model-setup.js';
import {Lifecycle} from './lifecycle.js';

const visibleSmoke = ARGV.includes('--smoke-test-visible');
const smoke = visibleSmoke || ARGV.includes('--smoke-test');
const app = new Gtk.Application({application_id: smoke ? 'io.github.zyuapp.JustSpeak.Smoke' : 'io.github.zyuapp.JustSpeak',
    // FLAGS_NONE is compatible with Ubuntu 22.04's GLib 2.72.
    flags: smoke ? Gio.ApplicationFlags.NON_UNIQUE : Gio.ApplicationFlags.FLAGS_NONE});
app.add_main_option('record-shortcut', 0, GLib.OptionFlags.NONE, GLib.OptionArg.NONE,
    'Open the shortcut recorder', null);
const backend = new Backend(smoke);
const lifecycle = new Lifecycle({backend, close: () => app.quit(), changed: pending => {
    stateRevision++;
    busy = pending;
    showNotice(pending ? 'Stopping JustSpeak…' : '');
    renderStatus();
}});
let window;
let recorder = null;
let pendingRecorder = false;
let smokeExit = 0;
let interval = 0;
let polling = null;
let starting = false;
let busy = false;
let rendering = false;
let checking = false;
let installing = false;
let stateRevision = 0;
let menuRequest = 0;
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
    result.connect('clicked', () => Promise.resolve().then(callback).catch(showError));
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
function showRefreshError(error = null) {
    controls.refreshError.label = error ? `Could not refresh settings and history: ${error.message || error}`.slice(0, 1200) : '';
    controls.refreshError.visible = Boolean(error);
}
function serviceBusy() { return ['recording', 'transcribing', 'canceling', 'updating', 'loading', 'stopping'].includes(status.phase); }
function canEdit() { return !busy && !installing && !serviceBusy() && status.phase !== 'disconnected' && Boolean(metadata.version); }
function capabilities() { return metadata.desktop || {}; }
function dictationPastes() { return capabilities().automatic_paste === true && metadata.settings.paste !== false; }

function renderStatus() {
    let text;
    switch (status.phase) {
    case 'recording': text = `Listening · ${Math.floor(status.elapsed_seconds || 0)}s`; break;
    case 'transcribing': text = 'Transcribing locally…'; break;
    case 'canceling': text = 'Canceling dictation…'; break;
    case 'loading': text = status.message || 'Loading speech model…'; break;
    case 'updating': text = status.message || 'Updating JustSpeak…'; break;
    case 'stopping': text = 'Stopping JustSpeak…'; break;
    case 'error': text = status.model_setup === 'required' ? 'Welcome to JustSpeak'
        : status.model_setup === 'failed' ? 'Model download needs attention'
        : status.message || 'JustSpeak needs attention'; break;
    case 'disconnected': text = 'JustSpeak is stopped'; break;
    default: text = status.model_ready ? 'Ready to dictate' : 'Speech model not ready';
    }
    controls.status.label = text;
    controls.modelSetup.render(status, busy || installing || Boolean(recorder));
    controls.start.visible = !controls.modelSetup.widget.visible;
    controls.desktop.visible = !controls.modelSetup.widget.visible;
    controls.start.label = status.phase === 'disconnected' ? 'Start JustSpeak'
        : status.phase === 'recording' ? 'Finish dictation' : 'Start dictation';
    controls.start.tooltip_text = dictationPastes()
        ? `Hides this window so your application receives dictation. Press and release ${status.shortcut || 'F10'} to finish, or reopen JustSpeak.`
        : 'Record here, then use Finish and paste the result from your clipboard.';
    controls.start.sensitive = !busy && !installing && (['disconnected', 'recording'].includes(status.phase)
        || (status.model_ready && ['idle', 'error'].includes(status.phase)));
    controls.cancel.visible = status.can_cancel === true;
    controls.cancel.sensitive = !busy;
    controls.input.sensitive = canEdit();
    controls.shortcut.sensitive = canEdit() && capabilities().shortcut_editing === true;
    controls.record.sensitive = controls.shortcut.sensitive;
    if (recorder && !recorder.saving && (serviceBusy() || status.phase === 'disconnected'))
        recorder.close('Shortcut recording canceled because JustSpeak is busy or stopped.');
    controls.clear.sensitive = canEdit() && metadata.history.length > 0;
    controls.history.sensitive = canEdit();
    controls.restart.sensitive = !busy && !installing && !serviceBusy();
    controls.quit.sensitive = !busy && !installing && !['updating', 'stopping'].includes(status.phase);
    controls.quit.label = lifecycle.quitting ? 'Quitting…' : 'Quit JustSpeak';
    controls.install.sensitive = !busy && !installing && !serviceBusy() && !checking;
    controls.check.sensitive = !busy && !installing && !checking;
    controls.refresh.sensitive = !busy && !installing;
    for (const [key, control] of switches) control.sensitive = canEdit()
        && (key !== 'paste' || capabilities().automatic_paste === true);
}

function renderMenu() {
    rendering = true;
    try {
        controls.version.label = metadata.version ? `JustSpeak ${metadata.version}` : 'JustSpeak';
        const desktop = capabilities();
        controls.desktop.label = desktop.shortcut_editing
            ? `Hold ${metadata.settings.shortcut || status.shortcut} in any application to dictate.`
            : 'Use Start and Finish here, then paste from the clipboard. Global shortcuts and automatic paste are not available on this desktop yet.';
        controls.experimental.visible = desktop.experimental === true;
        controls.shortcut.label = metadata.settings.shortcut || status.shortcut || 'F10';
        for (const [key, control] of switches) control.active = metadata.settings[key] === true;
        const selected = metadata.settings.input === null || metadata.settings.input === undefined ? 'default' : String(metadata.settings.input);
        const signature = JSON.stringify([metadata.inputs, metadata.settings.input]);
        if (signature !== inputSignature) {
            inputSignature = signature;
            const options = [{id: 'default', name: 'System default microphone'}, ...(metadata.inputs || [])];
            if (!options.some(option => String(option.id) === selected)) options.push({id: selected, name: `Unavailable input · ${selected}`});
            controls.inputIds = options.map(option => String(option.id));
            controls.input.model = Gtk.StringList.new(options.map(option => option.name + (option.is_default ? ' · default' : '')));
        }
        controls.input.selected = controls.inputIds.indexOf(selected);
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
        controls.historyScroll.visible = metadata.history.length > 0;
        // A fixed 70px minimum lets GTK crop even a short history list. Reserve
        // its natural height up to the existing limit, then scroll longer lists.
        const [, historyHeight] = controls.history.measure(Gtk.Orientation.VERTICAL, controls.history.get_width() || 480);
        controls.historyScroll.min_content_height = Math.max(70, Math.min(230, historyHeight));
    } finally {
        rendering = false;
    }
    renderStatus();
}

async function refreshMenu() {
    const revision = stateRevision;
    const request = ++menuRequest;
    const data = await backend.call(['menu', '--json'], {json: true});
    if (revision !== stateRevision || request !== menuRequest) return;
    if (!data.settings || !Array.isArray(data.inputs) || !Array.isArray(data.history)) throw new Error('Invalid settings response from JustSpeak.');
    data.history = data.history.slice(0, 10);
    metadata = data;
    lastMenu = Date.now();
    showRefreshError();
    renderMenu();
}
async function poll() {
    if (busy) return;
    if (polling) return polling;
    const revision = stateRevision;
    const previous = status.phase;
    polling = (async () => {
    try {
        try {
            const next = await backend.call(['status', '--json'], {json: true, timeout: 4000});
            if (revision !== stateRevision) return;
            status = next;
        } catch (error) {
            if (revision !== stateRevision) return;
            status = {phase: 'disconnected', model_ready: false, can_cancel: false, shortcut: status.shortcut};
        }
        renderStatus();
        if (status.phase !== 'disconnected' && (Date.now() - lastMenu > 8000 || previous !== status.phase)) {
            try { await refreshMenu(); } catch (error) { if (revision === stateRevision) showRefreshError(error); }
        }
        if (revision !== stateRevision) return;
        if (!smoke && status.phase === 'idle' && metadata.settings.auto_check_updates === true
            && Date.now() - lastUpdate > 6 * 60 * 60 * 1000) void checkUpdates(false);
    } finally {
        polling = null;
        // A response requested before an action cannot replace its newer state.
        // Arrange a fresh poll even when the action finished during that request.
        if (revision !== stateRevision && !busy) void poll().catch(showError);
        maybeOpenShortcutRecorder();
    }
    })();
    return polling;
}

function maybeOpenShortcutRecorder() {
    if (!pendingRecorder || starting || polling || busy || installing || status.phase === 'loading') return;
    pendingRecorder = false;
    openShortcutRecorder();
}

function openShortcutRecorder() {
    if (recorder) { recorder.present(); return; }
    if (!canEdit() || capabilities().shortcut_editing !== true) {
        showNotice(capabilities().shortcut_editing === false
            ? 'Shortcut recording is not available on this desktop yet.'
            : 'Start JustSpeak and finish any dictation before changing the shortcut.');
        return;
    }
    // A newly mapped parent can gain focus after its child and immediately
    // cancel the recorder. Wait for the parent's initial activation to settle.
    if (!window.is_active) { pendingRecorder = true; return; }
    showNotice('');
    recorder = new ShortcutRecorder({parent: window,
        current: metadata.settings.shortcut || status.shortcut,
        save: async shortcut => {
            try { await mutate(['shortcut', 'set', shortcut], 'Shortcut updated'); }
            catch (error) { showError(error); throw error; }
        },
        closed: message => { recorder = null; if (message) showNotice(message); },
    });
    recorder.present();
}
async function mutate(args, message = '', before = null) {
    if (busy || installing) return;
    stateRevision++;
    busy = true;
    showNotice(args[0] === 'shortcut' ? 'Saving shortcut…' : '');
    renderStatus();
    try {
        if (before) await before();
        await backend.call(args, {timeout: args[0] === 'shortcut' ? 25000 : 8000});
        // Once acknowledged, preserve the committed choice even if the service
        // goes away before the follow-up snapshot arrives.
        if (args[0] === 'settings') metadata.settings[args[2]] = args[3] === 'true';
        if (args[0] === 'input') metadata.settings.input = args[2] === 'default' ? null : args[2];
        if (args[0] === 'shortcut') metadata.settings.shortcut = args[2];
        if (args[0] === 'history' && args[1] === 'clear') metadata.history = [];
        showNotice(message);
        if (!['quit', 'launch', 'restart'].includes(args[0])) {
            try { await refreshMenu(); } catch (error) { showRefreshError(error); }
        }
        else lastMenu = 0;
    } finally {
        stateRevision++;
        // GTK changes the active switch/selection before the request completes.
        // Render the last confirmed values on failure as well as success.
        renderMenu();
        // Keep controls locked until pre-action requests have drained and a
        // fresh status tells us whether this command started more work.
        if (polling) await polling;
        busy = false;
        await poll();
        renderStatus();
    }
}
async function outsideAction(args) {
    try { await mutate(args, '', async () => { window.set_visible(false); await delay(200); }); }
    catch (error) { window.present(); throw error; }
}
async function checkUpdates(manual) {
    if (busy || checking || installing || (!manual && Date.now() - lastUpdate < 6 * 60 * 60 * 1000)) return;
    checking = true;
    lastUpdate = Date.now();
    controls.check.sensitive = false;
    controls.update.label = 'Checking for updates…';
    renderStatus();
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
    if (!update?.available || busy || installing || checking || serviceBusy()) return;
    stateRevision++;
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
        controls.update.label = `Update needs attention: ${error.message}`;
    } finally {
        stateRevision++;
        installing = false;
        controls.check.sensitive = true;
        renderStatus();
    }
}

function buildWindow(application) {
    window = new Gtk.ApplicationWindow({application, title: 'JustSpeak', icon_name: 'just-speak', default_width: 520, default_height: 760});
    window.connect('notify::is-active', () => {
        if (window.is_active) maybeOpenShortcutRecorder();
    });
    const header = new Gtk.HeaderBar();
    header.set_title_widget(label('JustSpeak'));
    controls.refresh = button('Refresh', async () => { lastMenu = 0; await poll(); });
    header.pack_end(controls.refresh);
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
    controls.modelSetup = new ModelSetup({setup: () => mutate(['model', 'setup']), onError: showError});
    content.append(controls.modelSetup.widget);
    controls.desktop = label('', {wrap: true, max_width_chars: 58});
    content.append(controls.desktop);
    controls.experimental = label('This desktop integration is experimental. GNOME behavior has not been verified.', {wrap: true});
    controls.experimental.add_css_class('dim-label');
    content.append(controls.experimental);
    controls.message = label('', {wrap: true, visible: false, selectable: true});
    content.append(controls.message);
    controls.refreshError = label('', {wrap: true, visible: false, selectable: true});
    controls.refreshError.add_css_class('error');
    content.append(controls.refreshError);
    controls.start = button('Start dictation', async () => {
        if (status.phase === 'disconnected') return mutate(['launch'], 'Starting JustSpeak');
        const command = status.phase === 'recording' ? 'stop' : 'start';
        if (dictationPastes()) return outsideAction([command]);
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
    controls.historyScroll = new Gtk.ScrolledWindow({hscrollbar_policy: Gtk.PolicyType.NEVER,
        min_content_height: 70, max_content_height: 230, propagate_natural_height: true});
    controls.historyScroll.set_child(controls.history);
    content.append(controls.historyScroll);
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
    controls.shortcut = label('F10', {hexpand: true});
    controls.shortcut.add_css_class('heading');
    controls.record = button('Record shortcut…', openShortcutRecorder);
    content.append(row(controls.shortcut, controls.record));
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
    controls.version = label('JustSpeak', {hexpand: true});
    controls.version.add_css_class('dim-label');
    controls.restart = button('Restart', () => mutate(['restart'], 'Restarting JustSpeak'));
    controls.quit = button('Quit JustSpeak', () => lifecycle.quit());
    content.append(row(controls.version, controls.restart, controls.quit));
    return window;
}

app.connect('activate', () => {
    if (window) { if (recorder) recorder.present(); else window.present(); void poll(); return; }
    buildWindow(app);
    renderMenu();
    if (smoke) {
        void (async () => {
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
        status = {phase: 'error', model_ready: false, model_setup: 'required'};
        renderStatus();
        if (!controls.modelSetup.widget.visible || !controls.modelSetup.button.sensitive || controls.start.visible)
            throw new Error('Missing model setup was not offered');
        for (const stage of ['downloading', 'verifying', 'extracting', 'loading']) {
            status = {phase: 'loading', model_ready: false, model_setup: stage, message: 'Setup progress'};
            renderStatus();
            if (!controls.modelSetup.spinner.spinning || controls.modelSetup.button.sensitive || controls.start.sensitive
                || controls.restart.sensitive || !controls.quit.sensitive || controls.install.sensitive || controls.input.sensitive)
                throw new Error(`Model setup mutation gate failed: ${stage}`);
        }
        status = {phase: 'error', model_ready: false, model_setup: 'failed', message: 'Download failed <b>plain text</b>'};
        renderStatus();
        if (!controls.modelSetup.button.sensitive || controls.modelSetup.button.label !== 'Retry download'
            || controls.modelSetup.details.use_markup || controls.modelSetup.spinner.spinning)
            throw new Error('Model setup retry state failed');
        status = {phase: 'idle', model_ready: true};
        renderStatus();
        if (controls.modelSetup.widget.visible || !controls.start.visible || !controls.start.sensitive)
            throw new Error('Model ready state did not enable dictation');
        status = {phase: 'disconnected', model_ready: false};
        renderStatus();
        if (controls.modelSetup.widget.visible || !controls.start.sensitive || controls.start.label !== 'Start JustSpeak')
            throw new Error('Stopped service cannot be started for setup');
        const {runWindowActions} = await import('./window-smoke.js');
        await runWindowActions({controls, switches, backend, renderMenu, renderStatus,
            mutate, poll, refreshMenu, checkUpdates, maybeOpenShortcutRecorder,
            get metadata() { return metadata; }, set metadata(value) { metadata = value; },
            get status() { return status; }, set status(value) { status = value; },
            set lastMenu(value) { lastMenu = value; },
            get pendingRecorder() { return pendingRecorder; }, set pendingRecorder(value) { pendingRecorder = value; },
            set starting(value) { starting = value; },
        });
        showNotice('');
        print('GTK_SMOKE_COMPLETE: window, history, settings, desktop capabilities, update and model setup gates, progress, retry; no external actions');
        } catch (error) { smokeExit = 1; printerr(`${error.message}\n${error.stack || ''}`); }
        if (visibleSmoke && smokeExit === 0) {
            metadata = JSON.parse(JSON.stringify(sampleMenu));
            status = {phase: 'idle', model_ready: true, can_cancel: false, shortcut: 'F10'};
            if (ARGV.includes('--smoke-model-setup')) {
                metadata.history = [];
                status = {phase: 'error', model_ready: false, model_setup: 'required', shortcut: 'F10'};
            }
            renderMenu();
            window.present();
            print('GTK_VISUAL_READY: synthetic content only');
            GLib.timeout_add(GLib.PRIORITY_DEFAULT, 8000, () => { app.quit(); return GLib.SOURCE_REMOVE; });
        } else {
            GLib.idle_add(GLib.PRIORITY_DEFAULT_IDLE, () => { app.quit(); return GLib.SOURCE_REMOVE; });
        }
        })();
        return;
    }
    window.present();
    starting = true;
    void (async () => {
        try {
            await poll();
            if (status.phase === 'disconnected') await mutate(['launch'], 'Starting JustSpeak');
        } finally {
            starting = false;
            maybeOpenShortcutRecorder();
        }
    })().catch(showError);
    interval = GLib.timeout_add(GLib.PRIORITY_DEFAULT, 1000, () => { void poll().catch(showError); return GLib.SOURCE_CONTINUE; });
});
const recordAction = new Gio.SimpleAction({name: 'record-shortcut'});
recordAction.connect('activate', () => { pendingRecorder = true; app.activate(); });
app.add_action(recordAction);
const quitAction = new Gio.SimpleAction({name: 'quit'});
quitAction.connect('activate', () => lifecycle.closeWindow());
app.add_action(quitAction);
app.connect('handle-local-options', (_application, options) => {
    if (!options.contains('record-shortcut')) return -1;
    // Actions deliver to the primary instance and return immediately. Retaining
    // GApplicationCommandLine in GJS can otherwise make launchers wait for GC.
    app.register(null);
    app.activate_action('record-shortcut', null);
    return app.get_is_remote() ? 0 : -1;
});
app.connect('shutdown', () => {
    if (recorder) recorder.close('', true);
    if (interval) GLib.source_remove(interval);
    backend.close();
});
const exitCode = app.run(smoke ? [] : ['just-speak', ...ARGV]);
System.exit(smoke ? smokeExit : exitCode);
