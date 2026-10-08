import init, { MidiPlayer } from './wasm/arpeg_wasm.js';
import { HeldKeys, noteName, orders, profileFor, typingKeys } from './music.mjs';
import { Performance } from './performance.mjs';
import { Synth } from './synth.mjs';

const elements = Object.fromEntries([...document.querySelectorAll('[id]')].map(e => [e.id, e]));
const held = new HeldKeys();
const keys = new Map();
let context;
let synth;
let performance;
let timer;
let animation;
let starting = false;

function config() {
    return {
        order: elements.order.value, latch: elements.latch.checked,
        step: elements.step.value, gate: Number(elements.gate.value),
        transpose: Number(elements.transpose.value),
    };
}

function settings() {
    return {
        waveform: elements.waveform.value,
        attack: Number(elements.attack.value) / 1000,
        decay: Number(elements.decay.value) / 1000,
        sustain: Number(elements.sustain.value) / 100,
        release: Number(elements.release.value) / 1000,
    };
}

function render() {
    const sounding = performance?.visibleNotes() ?? [];
    for (const [key, button] of keys) {
        button.classList.toggle('held', held.notes.includes(key));
        button.classList.toggle('sounding', sounding.includes(key));
        button.setAttribute('aria-pressed', String(held.notes.includes(key)));
    }
    elements.held.textContent = held.notes.map(noteName).join(' · ') || 'None';
    elements.sounding.textContent = sounding.map(noteName).join(' · ') || 'None';
}

function showTransport(running) {
    elements.play.textContent = running ? 'Stop' : context ? 'Start' : 'Start audio';
    elements.status.textContent = running ? 'Playing' : 'Ready to play';
    elements.lamp.classList.toggle('running', running);
}

function updateVisuals() {
    render();
    animation = requestAnimationFrame(updateVisuals);
}

function stop() {
    clearInterval(timer);
    cancelAnimationFrame(animation);
    timer = undefined;
    performance?.stop();
    for (const key of held.notes) performance?.dispatch('accept', new Uint8Array([128, key, 0]), 'notes');
    held.owners.clear();
    showTransport(false);
    render();
}

function report(error) {
    clearInterval(timer);
    cancelAnimationFrame(animation);
    timer = undefined;
    synth?.silence();
    // A failed WASM operation can leave the transport running. Rebuild before reuse.
    performance?.player.free();
    performance = undefined;
    held.owners.clear();
    showTransport(false);
    render();
    elements.error.hidden = false;
    elements.error.textContent = String(error);
}

function perform(action) {
    try { action(); } catch (error) { report(error); }
}

function run() {
    performance.dispatch('accept', new Uint8Array([251]), 'clock');
    performance.dispatch('advance');
    timer = setInterval(() => perform(() => performance.dispatch('advance')), 5);
    showTransport(true);
    updateVisuals();
}

function rebuild() {
    if (!context) return;
    const running = timer !== undefined;
    clearInterval(timer);
    cancelAnimationFrame(animation);
    timer = undefined;
    performance?.dispose();
    performance = new Performance(MidiPlayer, profileFor(config()), Number(elements.tempo.value), context, synth, settings);
    sendBend();
    for (const key of held.notes) {
        performance.dispatch('accept', new Uint8Array([144, key, Number(elements.velocity.value)]), 'notes');
    }
    if (running) run();
    render();
}

function press(owner, key) {
    if (!performance || starting) return;
    if (held.press(owner, key)) {
        performance.dispatch('accept', new Uint8Array([144, key, Number(elements.velocity.value)]), 'notes');
        performance.dispatch('advance');
    }
    render();
}

function release(owner) {
    const key = held.release(owner);
    if (key !== null && performance) {
        performance.dispatch('accept', new Uint8Array([128, key, 0]), 'notes');
    }
    render();
}

function sendBend() {
    if (!performance) return;
    const value = Number(elements.bend.value) / 100;
    const midi = Math.round(8192 + value * (value < 0 ? 8192 : 8191));
    performance.dispatch('accept', new Uint8Array([224, midi & 127, midi >> 7]), 'notes');
}

for (const [value, name] of Object.entries(orders)) elements.order.add(new Option(name, value));

let whites = 0;
for (let key = 48; key <= 72; key += 1) {
    const black = [1, 3, 6, 8, 10].includes(key % 12);
    const button = document.createElement('button');
    button.type = 'button';
    button.className = `key ${black ? 'black' : 'white'}`;
    button.disabled = true;
    button.setAttribute('aria-label', noteName(key));
    button.setAttribute('aria-pressed', 'false');
    if (black) button.style.left = `${whites / 15 * 100 - 2.1}%`;
    else whites += 1;
    const label = document.createElement('span');
    label.textContent = noteName(key);
    button.append(label);
    const code = [...typingKeys].find(([, note]) => note === key)?.[0];
    if (code) {
        const shortcut = document.createElement('small');
        shortcut.textContent = code.slice(3);
        button.append(shortcut);
    }
    button.addEventListener('pointerdown', event => {
        if (event.button !== 0) return;
        event.preventDefault();
        button.focus();
        button.setPointerCapture(event.pointerId);
        perform(() => press(`pointer:${event.pointerId}`, key));
    });
    button.addEventListener('lostpointercapture', event => perform(() => release(`pointer:${event.pointerId}`)));
    button.addEventListener('pointerup', event => perform(() => release(`pointer:${event.pointerId}`)));
    button.addEventListener('pointercancel', event => perform(() => release(`pointer:${event.pointerId}`)));
    button.addEventListener('keydown', event => {
        if (!['Space', 'Enter'].includes(event.code)) return;
        event.preventDefault();
        if (!event.repeat) perform(() => press(`button:${key}`, key));
    });
    button.addEventListener('keyup', event => {
        if (!['Space', 'Enter'].includes(event.code)) return;
        event.preventDefault();
        perform(() => release(`button:${key}`));
    });
    button.addEventListener('blur', () => perform(() => release(`button:${key}`)));
    elements.keyboard.append(button);
    keys.set(key, button);
}

document.addEventListener('keydown', event => {
    if (!typingKeys.has(event.code) || event.repeat || event.ctrlKey || event.metaKey || event.altKey ||
        event.target.closest('input, select, textarea, button:not(.key)')) return;
    event.preventDefault();
    perform(() => press(event.code, typingKeys.get(event.code)));
});
document.addEventListener('keyup', event => {
    if (typingKeys.has(event.code)) perform(() => release(event.code));
});

elements.play.addEventListener('click', async () => {
    if (starting) return;
    if (timer !== undefined) { perform(stop); return; }
    starting = true;
    elements.play.disabled = true;
    try {
        context ??= new AudioContext({ latencyHint: 'interactive' });
        await context.resume();
        if (document.hidden || !document.hasFocus()) return;
        synth ??= new Synth(context);
        synth.master.gain.value = Number(elements.volume.value) / 100;
        if (!performance) rebuild();
        elements.error.hidden = true;
        for (const button of keys.values()) button.disabled = false;
        run();
    } catch (error) { report(error); }
    finally { starting = false; elements.play.disabled = false; }
});

elements.clear.addEventListener('click', () => perform(() => {
    for (const key of held.notes) performance?.dispatch('accept', new Uint8Array([128, key, 0]), 'notes');
    held.owners.clear();
    if (elements.latch.checked) performance?.dispatch('clear');
    synth?.silence();
    if (performance) {
        performance.visualNotes = [];
        performance.sounding.clear();
    }
    render();
}));
for (const name of ['order', 'latch', 'step']) elements[name].addEventListener('change', () => perform(rebuild));
elements.tempo.addEventListener('change', () => perform(() => {
    if (!elements.tempo.checkValidity()) { elements.tempo.reportValidity(); return; }
    performance?.tempo(Number(elements.tempo.value));
}));

for (const name of ['gate', 'transpose', 'velocity', 'bend', 'volume', 'attack', 'decay', 'sustain', 'release']) {
    elements[name].addEventListener('input', () => perform(() => {
        const value = elements[name].value;
        const unit = ['gate', 'volume', 'sustain'].includes(name) ? '%' : ['attack', 'decay', 'release'].includes(name) ? ' ms' : '';
        elements[`${name}-value`].textContent = `${value}${unit}`;
        if (name === 'volume' && synth) synth.master.gain.setTargetAtTime(Number(value) / 100, context.currentTime, 0.01);
        if (name === 'gate') performance?.dispatch('control', 'gate', `${value}/100`);
        if (name === 'transpose') performance?.dispatch('control', 'transposition', value);
        if (name === 'bend') sendBend();
    }));
}

window.addEventListener('blur', () => perform(stop));
document.addEventListener('visibilitychange', () => { if (document.hidden) perform(stop); });
window.addEventListener('pagehide', () => perform(stop));

try {
    await init();
    elements.controls.disabled = false;
    elements['sound-controls'].disabled = false;
    elements.clear.disabled = false;
    elements.play.disabled = false;
    showTransport(false);
} catch (error) {
    elements.status.textContent = 'Instrument unavailable';
    elements.error.hidden = false;
    elements.error.textContent = `Could not load the instrument: ${error}. Build and serve the web directory as described in the README.`;
}
