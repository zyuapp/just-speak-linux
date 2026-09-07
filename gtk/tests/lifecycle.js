import {Lifecycle} from '../lifecycle.js';

function check(condition, message) { if (!condition) throw new Error(message); }

let finish;
let calls = 0;
let closes = 0;
const pending = [];
const backend = {call: async (args, options) => {
    check(args.join(' ') === 'quit' && options.timeout > 30000, 'Wrong shutdown request/deadline');
    calls++;
    await new Promise(resolve => { finish = resolve; });
}};
const lifecycle = new Lifecycle({backend, close: () => closes++, changed: value => pending.push(value)});
const quit = lifecycle.quit();
check(lifecycle.quitting && closes === 0, 'Window closed before shutdown completed');
await lifecycle.quit();
lifecycle.closeWindow();
check(calls === 1 && closes === 0, 'Duplicate Quit or remote callback interrupted shutdown');
finish();
await quit;
check(closes === 1, 'Successful Quit did not close the window');

backend.call = async () => { throw new Error('Fixture: update is still running'); };
const retry = new Lifecycle({backend, close: () => closes++, changed: value => pending.push(value)});
try { await retry.quit(); throw new Error('Failed Quit reported success'); }
catch (error) { check(error.message.includes('Fixture'), 'Quit error was hidden'); }
check(!retry.quitting && closes === 1 && pending.at(-1) === false, 'Failed Quit closed the window or blocked retry');
backend.call = async () => {};
await retry.quit();
check(closes === 2, 'Retry did not close the window');
new Lifecycle({backend, close: () => closes++}).closeWindow();
check(closes === 3, 'A Quit from the bar did not close the open window');
print('PASS GTK Quit: completion, duplicate clicks, remote callback, failure, retry, bar-initiated close');
