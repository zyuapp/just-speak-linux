import Gio from 'gi://Gio';
import GLib from 'gi://GLib';

Gio._promisify(Gio.Subprocess.prototype, 'communicate_utf8_async');

export const sampleMenu = {
    version: '0.2.0',
    settings: {input: null, shortcut: 'F10', sound_feedback: true, mute_while_recording: true,
        paste: true, history_enabled: true, auto_check_updates: false},
    inputs: [{id: '55', name: 'Built-in microphone', is_default: true}],
    input_error: null,
    desktop: {name: 'hyprland', automatic_paste: true, shortcut_editing: true, experimental: false},
    history: [{id: 'smoke-one', text: 'A sample transcript. <b>This is plain text.</b>', created_at: 1788750000}],
};

export class Backend {
    constructor(smoke = false) {
        this.smoke = smoke;
        this.children = new Set();
        const local = GLib.build_filenamev([GLib.get_home_dir(), '.local', 'bin', 'just-speak']);
        this.executable = GLib.getenv('JUST_SPEAK_BIN') ||
            (GLib.file_test(local, GLib.FileTest.IS_EXECUTABLE) ? local : 'just-speak');
    }

    async call(args, {json = false, timeout = 8000} = {}) {
        if (this.smoke) {
            if (args[0] === 'menu') return JSON.parse(JSON.stringify(sampleMenu));
            if (args[0] === 'status') return {phase: 'idle', model_ready: true, shortcut: 'F10', can_cancel: false};
            throw new Error('Smoke mode never performs actions or network requests');
        }
        const process = Gio.Subprocess.new([this.executable, ...args],
            Gio.SubprocessFlags.STDOUT_PIPE | Gio.SubprocessFlags.STDERR_PIPE);
        this.children.add(process);
        const cancel = new Gio.Cancellable();
        let timedOut = false;
        const timer = timeout > 0 ? GLib.timeout_add(GLib.PRIORITY_DEFAULT, timeout, () => {
            timedOut = true;
            process.force_exit();
            cancel.cancel();
            return GLib.SOURCE_REMOVE;
        }) : 0;
        try {
            const [stdout, stderr] = await process.communicate_utf8_async(null, cancel);
            if (!process.get_successful()) throw new Error((stderr || 'JustSpeak could not complete the action.').trim().slice(0, 1200));
            if ((stdout || '').length > 2 * 1024 * 1024) throw new Error('JustSpeak returned too much data.');
            return json ? JSON.parse(stdout) : stdout;
        } catch (error) {
            if (timedOut) throw new Error('JustSpeak took too long to respond. Try again.');
            throw error;
        } finally {
            if (timer && !timedOut) GLib.source_remove(timer);
            this.children.delete(process);
        }
    }

    close() {
        for (const process of this.children) process.force_exit();
        this.children.clear();
    }
}

export function delay(milliseconds) {
    return new Promise(resolve => GLib.timeout_add(GLib.PRIORITY_DEFAULT, milliseconds, () => {
        resolve();
        return GLib.SOURCE_REMOVE;
    }));
}
