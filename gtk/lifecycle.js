// A Quit request owns its CLI until completion. The CLI also asks any existing
// window to close, so ignore that callback while this window is awaiting Quit.
export class Lifecycle {
    constructor({backend, close, changed = () => {}}) {
        this.backend = backend;
        this.close = close;
        this.changed = changed;
        this.quitting = false;
    }

    closeWindow() {
        if (!this.quitting) this.close();
    }

    async quit() {
        if (this.quitting) return;
        this.quitting = true;
        this.changed(true);
        try {
            await this.backend.call(['quit'], {timeout: 35000});
        } catch (error) {
            this.quitting = false;
            this.changed(false);
            throw error;
        }
        this.close();
    }
}
