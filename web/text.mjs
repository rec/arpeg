import init, { TokenPlayer } from './wasm/arpeg_wasm.js';
import { TextKeyboard } from './text_keyboard.mjs';

const elements = Object.fromEntries([...document.querySelectorAll('[id]')].map(e => [e.id, e]));
let keyboard;
let timer;
let origin;

function now() { return BigInt(Math.floor((performance.now() - origin) * 1000)); }
function append(tokens) { elements.output.textContent += tokens.join(''); }

function stop() {
    clearInterval(timer);
    timer = undefined;
    if (keyboard) {
        try { append(keyboard.stop(now())); } finally { keyboard.player.free(); keyboard = undefined; }
    }
    elements.start.textContent = 'Start';
    elements.order.disabled = false;
    elements.latch.disabled = false;
    elements.status.textContent = 'Stopped';
}

function perform(action) {
    try { action(); } catch (error) { stop(); elements.status.textContent = String(error); }
}

elements.start.addEventListener('pointerdown', event => { if (keyboard) event.preventDefault(); });

elements.start.addEventListener('click', () => perform(() => {
    if (keyboard) { stop(); return; }
    origin = performance.now();
    keyboard = new TextKeyboard(new TokenPlayer(elements.order.value, elements.latch.checked, '1/4', '4/5'));
    elements.start.textContent = 'Stop';
    elements.order.disabled = true;
    elements.latch.disabled = true;
    elements.status.textContent = 'Hold keys in the keyboard input area';
    elements.capture.focus();
    timer = setInterval(() => perform(() => append(keyboard.advance(now()))), 10);
}));

elements.capture.addEventListener('keydown', event => {
    if (!keyboard || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || [...event.key].length !== 1) return;
    event.preventDefault();
    perform(() => append(keyboard.press(now(), event.code, event.key)));
});

elements.capture.addEventListener('keyup', event => {
    if (!keyboard) return;
    perform(() => append(keyboard.release(now(), event.code)));
});

elements.capture.addEventListener('blur', () => perform(stop));
window.addEventListener('pagehide', () => perform(stop));
elements.clear.addEventListener('click', () => { elements.output.textContent = ''; });

try { await init(); elements.start.disabled = false; elements.status.textContent = 'Ready'; }
catch (error) { elements.status.textContent = `Could not load the text engine: ${error}`; }
