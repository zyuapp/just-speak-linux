# App workflow review — September 7, 2026

The Rust service and separate desktop interfaces are a reasonable foundation
for resident, offline dictation. The recurring weakness was an incomplete
contract for action completion: the service, CLI, GTK window and Omarchy popup
could disagree about whether an action had finished or failed. Missing workflow
tests allowed those disagreements to reach the user. These are specific,
confirmed failure paths; this review does not establish the cause of every
reported bug or certify every desktop integration.

## Confirmed failures and fixes

| User action or trigger | Failure | Result after this change |
| --- | --- | --- |
| Quit from GTK | Service stopped but the window stayed open | GTK retains the Quit request until cleanup completes, then closes |
| Quit from the bar | Popup hid errors, icon remained, menu refreshed against the stopped service | Errors remain visible; successful Quit closes GTK and removes the icon and its slot |
| Quit during dictation or model setup | UI/service rejected Quit while work was active | `stopping` cancels work and waits for cleanup; shortcut saving and update installation still explain why Quit is blocked |
| Quit with a partially written socket request | A successful reply could precede a lingering process; reply delivery also raced exit | Unfinished clients are interrupted, other clients joined, socket/lock released and reply sent before exit |
| Reopen after Quit | Opening GTK did not restart a stopped service | A new window starts the service and restores its current state |
| Failed preference or microphone change | Control retained an uncommitted value | GTK immediately restores the last confirmed value and shows the failure |
| Successful action followed by failed refresh | UI reported the action as failed | Action acknowledgement and refresh errors are displayed separately |
| A slow pre-action status/menu response finishes later | Old data could overwrite the action's new state | Both interfaces invalidate their own outstanding pre-action reads |
| Manual dictation with automatic paste disabled | GTK hid the Start/Finish controls unnecessarily | The controls stay visible; focus handoff is reserved for automatic paste |
| Repeated click during focus handoff | Controls were not locked during the hide delay | The pending action is locked before the delay |
| Open the shortcut recorder during startup | Launch/loading could consume the pending request | The recorder opens when startup finishes and editing becomes available |
| Start or history paste fails after closing the popup | Error was left in a hidden menu | The popup reopens with the failure; opening the menu does not erase it |
| A bar command cannot start | Quickshell did not emit normal process completion, leaving controls busy | A shared process helper detects failed startup and releases the controls |
| Bar command timeout or malformed update response | Failure could disappear or be rendered as success | Explicit failures persist and retry remains available |
| Cancel and immediately start again | Service published idle before audio/feedback cleanup finished | `canceling` keeps new work blocked until cleanup completes |
| Recorder dies while listening | UI remained in recording until the user stopped it | Service detects the recorder exit, reports its error and cleans up for retry |
| Update install/restart fails | Service could remain stuck in `updating` | A scoped update guard releases the gate on failure and protects the replacement daemon |
| Short transcript history | Inner scroller clipped the second row despite unused outer space | Short lists reserve their measured height; empty lists hide the scroller |

## Verification

- **Core checks:** formatting, Clippy with warnings denied, build, shell/Lua
  syntax, and Rust tests: 66 passed, one optional test ignored.
- **Real CLI/daemon lifecycle:** stopped and invalid-config Quit, rejected Quit
  during update, download cancellation, GTK close delivery, a partial socket
  client, 12 complete Quit replies and watcher disconnects. The partial-client
  regression failed against the earlier implementation and passed after the fix.
- **Actual GTK window + real CLI and daemon:** tested on Wayland and X11 with a
  private D-Bus session and fake service/download helpers. Opening starts the
  service; close preserves it; reopen reconnects; Restart replaces it; external
  Quit closes GTK; setup survives close/reopen; the actual Quit handler stops
  setup and completes service cleanup.
- **GTK widgets:** actual switch, microphone, dictation, history and update
  handlers with a fake backend, including rollback/retry, delayed responses,
  pending-action locks, startup shortcut intent and model setup gates. Visual
  inspection used synthetic transcripts in a separate app instance and checked
  wrapping, history height, empty state and visible failures. Shortcut recorder
  logic and widget regression suites also passed.
- **Omarchy popup:** shortcut success and denied/malformed/timeout cases,
  lifecycle and visibility, plus 34 workflow stages covering stale menu reads,
  hidden-action failures, failed executable startup and update failure/retry.
  These use the real QML components with a fake backend and shortened deadlines.
- **Audio path:** real `pw-record` on a private synthetic PipeWire server produced
  mono 16 kHz PCM16 and shut down promptly. The real-model smoke test passed
  inference, recorder failure/retry, delayed cancellation cleanup and Quit with
  synthetic input and fake clipboard/paste/feedback helpers.

`make check` runs the core and isolated lifecycle checks. `make check-window`
requires GTK4/GJS and a display; `make check-panel` requires Omarchy/Quickshell.
`make smoke` needs the speech model, and `make pipewire-test` needs PipeWire.
The review did not install a release, change desktop configuration, use the
user's microphone/clipboard, or run a real update installation. Physical input
devices, real paste targets and GNOME remain outside this pass's coverage.

## Remaining architecture work

1. **Publish allowed actions and blocking reasons from the service.** GTK and
   QML still duplicate phase-based enablement. The new cancellation/stopping
   phases close confirmed gaps, but a shared action contract would reduce future
   drift.
2. **Use a service-wide state revision or one complete UI subscription.** The
   new frontend revisions reject stale reads around that frontend's actions.
   Simultaneous changes from another client can still race independent status
   and menu snapshots.
3. **Give operations explicit kinds and completion semantics.** `updating`
   still represents both application installation and shortcut saving. Keep
   cleanup completion part of the operation state, rather than inferring it
   from whether a command was accepted.
4. **Define irreversible clipboard completion precisely.** Cancellation suppresses
   pending paste, but an already-started `wl-copy` can finish changing the
   clipboard. This existing limitation is not an undo guarantee.

These are incremental changes to a shared service contract. Replacing a toolkit
or language would still require resolving action ownership and completion.
