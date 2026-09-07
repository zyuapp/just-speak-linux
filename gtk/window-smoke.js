import {delay, sampleMenu} from './backend.js';

function check(condition, message) { if (!condition) throw new Error(message); }
function clone(value) { return JSON.parse(JSON.stringify(value)); }
function deferred() {
    let resolve, reject;
    const promise = new Promise((yes, no) => { resolve = yes; reject = no; });
    return {promise, resolve, reject};
}
async function settle() { await delay(1); }

// Runs against the real window controls. Every command is replaced before a
// button is clicked; no service, settings, clipboard or desktop action is used.
export async function runWindowActions(view) {
    const {controls, switches, backend} = view;
    const originalCall = backend.call;
    const ready = {phase: 'idle', model_ready: true, can_cancel: false, shortcut: 'F10'};
    let saved = clone(sampleMenu);
    const reset = () => {
        view.metadata = clone(saved);
        view.status = clone(ready);
        view.lastMenu = Date.now();
        view.renderMenu();
    };
    try {
        reset();
        check(controls.input.sensitive && controls.clear.sensitive && controls.history.sensitive
            && controls.quit.sensitive && controls.restart.sensitive && controls.check.sensitive
            && controls.refresh.sensitive && switches.get('sound_feedback').sensitive,
        'Idle settings, history or lifecycle controls are disabled');

        view.metadata.history = Array.from({length: 2}, (_unused, id) =>
            ({id: String(id), text: 'First line\nSecond line\nThird line', created_at: 1788750000}));
        view.renderMenu();
        check(controls.historyScroll.min_content_height > 140,
            'A short history list is compressed to a partly clipped row');
        view.metadata.history = [];
        view.renderMenu();
        check(!controls.historyScroll.visible && controls.empty.visible,
            'Empty history leaves an unused scrolling area');
        reset();

        // A rejected write must roll back immediately, without waiting for the
        // next periodic menu refresh (which can itself fail).
        let request = deferred();
        backend.call = async args => args[0] === 'status' ? clone(ready) : request.promise;
        switches.get('sound_feedback').active = false;
        check(!controls.input.sensitive && !controls.check.sensitive, 'Pending settings did not lock conflicting controls');
        request.reject(new Error('Fixture: settings rejected'));
        await settle();
        check(switches.get('sound_feedback').active && switches.get('sound_feedback').state,
            'Rejected preference still looks changed');
        check(controls.message.label.includes('settings rejected'), 'Rejected preference error was lost');

        request = deferred();
        controls.input.selected = 1;
        request.reject(new Error('Fixture: microphone rejected'));
        await settle();
        check(controls.input.selected === 0, 'Rejected microphone selection was not restored');

        backend.call = async args => {
            if (args[0] === 'status') return clone(ready);
            if (args[0] === 'menu') throw new Error('Fixture: snapshot unavailable');
            return '';
        };
        await view.mutate(['shortcut', 'set', 'F9'], 'Shortcut updated');
        check(controls.shortcut.label === 'F9' && controls.message.label === 'Shortcut updated'
            && controls.refreshError.label.includes('snapshot unavailable'),
        'A successful save was lost or reported as failed when refresh failed');
        await view.mutate(['settings', 'set', 'sound_feedback', 'false'], 'Settings saved');
        check(!switches.get('sound_feedback').active && !switches.get('sound_feedback').state,
            'Acknowledged preference reverted after refresh failure');

        reset();
        const oldStatus = deferred();
        let statusCalls = 0;
        backend.call = async args => {
            if (args[0] === 'status') return ++statusCalls === 1 ? oldStatus.promise : clone(ready);
            if (args[0] === 'menu') return clone(saved);
            return '';
        };
        const oldPoll = view.poll();
        const save = view.mutate(['settings', 'set', 'sound_feedback', 'true']);
        await settle();
        check(!controls.start.sensitive, 'Action unlocked controls before the fresh status arrived');
        oldStatus.resolve({phase: 'recording', model_ready: true, can_cancel: true});
        await Promise.all([oldPoll, save]);
        await settle();
        check(view.status.phase === 'idle' && statusCalls === 2 && controls.input.sensitive,
            'A pre-action poll overwrote newer state or prevented a fresh poll');

        const oldMenu = deferred();
        let menuCalls = 0;
        backend.call = async args => {
            if (args[0] === 'status') return clone(ready);
            if (args[0] === 'menu') return ++menuCalls === 1 ? oldMenu.promise : clone(saved);
            return '';
        };
        const oldRefresh = view.refreshMenu();
        saved.settings.shortcut = 'F8';
        await view.mutate(['shortcut', 'set', 'F8'], 'Shortcut updated');
        oldMenu.resolve(clone(sampleMenu));
        await oldRefresh;
        check(controls.shortcut.label === 'F8' && !controls.refreshError.visible,
            'A stale menu response overwrote the saved shortcut');

        const handoff = deferred();
        let actionCalls = 0;
        backend.call = async args => {
            if (args[0] === 'status') return clone(ready);
            if (args[0] === 'menu') return clone(saved);
            actionCalls++;
            return '';
        };
        const starting = view.mutate(['start'], '', () => handoff.promise);
        await view.mutate(['start']);
        check(actionCalls === 0 && !controls.start.sensitive,
            'Focus handoff failed to reserve the action before the command starts');
        handoff.resolve();
        await starting;
        check(actionCalls === 1, 'Focus handoff allowed a duplicate action');

        saved.settings.paste = false;
        reset();
        actionCalls = 0;
        controls.start.emit('clicked');
        await settle();
        check(actionCalls === 1 && controls.message.label === 'Listening'
            && controls.start.tooltip_text.includes('clipboard'),
        'Clipboard-only dictation still hides the window for automatic paste');

        view.pendingRecorder = true;
        view.starting = true;
        view.status = {phase: 'disconnected', model_ready: false};
        view.maybeOpenShortcutRecorder();
        check(view.pendingRecorder, 'Shortcut launch intent was dropped before service startup');
        view.starting = false;
        view.status = {phase: 'loading', model_ready: false};
        view.maybeOpenShortcutRecorder();
        check(view.pendingRecorder, 'Shortcut launch intent was dropped during model loading');
        view.pendingRecorder = false;
        reset();

        const checking = deferred();
        backend.call = async () => checking.promise;
        const updateCheck = view.checkUpdates(true);
        check(!controls.check.sensitive && !controls.install.sensitive,
            'Update check permits duplicate checks or installation with stale release data');
        checking.reject(new Error('Fixture: offline'));
        await updateCheck;
        check(controls.check.sensitive && controls.update.label.includes('offline'),
            'Failed update check hides the error or prevents retry');

        backend.call = async args => {
            if (args[1] === 'check') return {available: true, latest_version: '0.2.99'};
            throw new Error('Fixture: installation rejected');
        };
        await view.checkUpdates(true);
        controls.install.emit('clicked');
        await settle();
        check(controls.update.label.includes('installation rejected') && controls.install.visible
            && controls.install.sensitive && controls.check.sensitive && controls.quit.sensitive,
        'Failed update installation loses its error or leaves controls locked');

        view.status = {phase: 'canceling', model_ready: true, can_cancel: false};
        view.renderStatus();
        check(!controls.start.sensitive && !controls.input.sensitive && !controls.install.sensitive
            && !controls.restart.sensitive && controls.quit.sensitive && !controls.cancel.visible
            && controls.status.label === 'Canceling dictation…',
        'Cancel cleanup exposes actions before the microphone is ready');
        print('GTK_ACTIONS_COMPLETE: rejected settings rollback, acknowledged writes, stale responses, focus handoff, clipboard-only controls, startup shortcut intent, update retry, cancel cleanup; no external actions');
    } finally {
        backend.call = originalCall;
        view.metadata = clone(sampleMenu);
        view.status = clone(ready);
        view.pendingRecorder = false;
        view.starting = false;
        controls.update.label = 'Check for new releases of JustSpeak.';
        controls.install.visible = false;
        view.renderMenu();
    }
}
