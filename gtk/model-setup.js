import Gtk from 'gi://Gtk?version=4.0';

// Setup belongs to the service. This view can be closed and recreated while a
// download is running, without owning a downloader process or a local timer.
export class ModelSetup {
    constructor({setup, onError}) {
        this.widget = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 12, visible: false});
        this.widget.add_css_class('card');
        const content = new Gtk.Box({orientation: Gtk.Orientation.VERTICAL, spacing: 10,
            margin_top: 16, margin_bottom: 16, margin_start: 16, margin_end: 16});
        this.widget.append(content);
        const title = new Gtk.Label({label: 'Set up offline dictation', xalign: 0});
        title.add_css_class('heading');
        content.append(title);
        content.append(new Gtk.Label({
            label: 'Download the English speech model once to turn your voice into text. After setup, dictation works offline.',
            xalign: 0, wrap: true, max_width_chars: 52,
        }));
        content.append(new Gtk.Label({
            label: 'Parakeet · ~483 MB download · ~661 MB installed\nDownloads from GitHub. No account needed.',
            xalign: 0, wrap: true, max_width_chars: 52, css_classes: ['dim-label'],
        }));
        this.details = new Gtk.Label({xalign: 0, wrap: true, max_width_chars: 52,
            selectable: true, use_markup: false, visible: false});
        content.append(this.details);
        const actions = new Gtk.Box({orientation: Gtk.Orientation.HORIZONTAL, spacing: 10});
        this.spinner = new Gtk.Spinner({visible: false});
        actions.append(this.spinner);
        this.button = new Gtk.Button({label: 'Download model (~483 MB)', halign: Gtk.Align.START});
        this.button.add_css_class('suggested-action');
        this.button.connect('clicked', () => Promise.resolve().then(setup).catch(onError));
        actions.append(this.button);
        content.append(actions);
    }

    render(status, blocked = false) {
        const state = status.model_setup;
        this.widget.visible = Boolean(state) && !status.model_ready && status.phase !== 'disconnected';
        const active = ['downloading', 'verifying', 'extracting', 'loading'].includes(state);
        this.spinner.visible = this.widget.visible && active;
        this.spinner.spinning = this.spinner.visible;
        this.button.sensitive = !blocked && ['required', 'failed'].includes(state)
            && ['idle', 'error'].includes(status.phase);
        this.button.label = active ? 'Setting up…' : state === 'failed' ? 'Retry download' : 'Download model (~483 MB)';
        this.details.label = state === 'failed'
            ? `${status.message || 'The download could not finish.'}\nYou can retry when you’re ready.`
            : active ? `${status.message || 'Setting up the speech model…'}\nYou can close this window. Setup will continue in the background.` : '';
        this.details.visible = Boolean(this.details.label);
        if (state === 'failed') this.details.add_css_class('error');
        else this.details.remove_css_class('error');
    }
}
