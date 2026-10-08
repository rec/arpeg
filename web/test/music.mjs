import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { initSync, MidiPlayer } from '../../dist/wasm/arpeg_wasm.js';
import { audioTime, HeldKeys, orders, profileFor } from '../music.mjs';
import { Performance } from '../performance.mjs';
import { envelopeLevel } from '../synth.mjs';

initSync({ module: readFileSync(new URL('../../dist/wasm/arpeg_wasm_bg.wasm', import.meta.url)) });

const config = { order: 'ascending', latch: false, step: '1/4', gate: 80, transpose: 0 };

function instrument(overrides = {}) {
    const context = { currentTime: 10 };
    const output = [];
    const synth = {
        note: (event, at) => output.push({ ...event, at }),
        pitchBend: (data, at) => output.push({ bend: [...data], at }),
        silence: () => output.push({ silence: true }),
    };
    const performance = new Performance(MidiPlayer, profileFor({ ...config, ...overrides }), 120, context, synth, () => ({}));
    performance.dispatch('accept', new Uint8Array([251]), 'clock');
    return { performance, context, output };
}

test('every offered order plays through the existing WASM engine', () => {
    for (const order of Object.keys(orders)) {
        const { performance, context, output } = instrument({ order });
        try {
            for (const key of [60, 64, 67]) performance.dispatch('accept', new Uint8Array([144, key, 100]), 'notes');
            performance.dispatch('advance');
            context.currentTime += 0.125;
            performance.dispatch('advance');
            assert.equal(output.filter(e => e.port === 'note_start').length, 2, order);
            assert.ok(output.every(e => [60, 64, 67].includes(e.key)), order);
        } finally { performance.dispose(); }
    }
});

test('pointer and typing ownership cannot release each other’s held note', () => {
    const held = new HeldKeys();
    assert.equal(held.press('KeyA', 60), true);
    assert.equal(held.press('KeyA', 60), false);
    assert.equal(held.press('pointer:1', 60), false);
    assert.equal(held.press('KeyD', 64), true);
    assert.deepEqual(held.notes, [60, 64]);
    assert.equal(held.release('KeyA'), null);
    assert.equal(held.release('missing'), null);
    assert.equal(held.release('pointer:1'), 60);
    assert.deepEqual(held.notes, [64]);
});

test('irregular polling retains exact attack and gate spacing in the audio timeline', () => {
    const { performance, context, output } = instrument();
    try {
        performance.dispatch('accept', new Uint8Array([144, 60, 100]), 'notes');
        performance.dispatch('advance');
        context.currentTime = 10.130;
        performance.dispatch('advance');
        const notes = output.filter(e => e.port);
        assert.deepEqual(notes.map(e => e.port), ['note_start', 'note_end', 'note_start']);
        for (const [index, expected] of [10.040, 10.140, 10.165].entries()) {
            assert.ok(Math.abs(notes[index].at - expected) < 1e-9);
        }
        context.currentTime = 10.039;
        assert.deepEqual(performance.visibleNotes(), []);
        context.currentTime = 10.041;
        assert.deepEqual(performance.visibleNotes(), [60]);
    } finally { context.currentTime = 10.130; performance.dispose(); }
});

test('late notes clamp to current audio time rather than scheduling in the past', () => {
    assert.equal(audioTime('1/4', '1', 120, 10), 10);
});

test('tempo changes flush old gates using the old tempo and schedule new steps using the new tempo', () => {
    const { performance, context, output } = instrument();
    try {
        performance.dispatch('accept', new Uint8Array([144, 60, 100]), 'notes');
        performance.dispatch('advance');
        context.currentTime = 10.110;
        performance.tempo(60);
        assert.ok(Math.abs(output[1].at - 10.140) < 1e-9);
        context.currentTime = 10.145;
        performance.dispatch('advance');
        assert.ok(Math.abs(output[2].at - 10.180) < 1e-9);
    } finally { performance.dispose(); }
});

test('latch survives source release and stop, then resumes without a ghost sounding display', () => {
    const { performance, context, output } = instrument({ latch: true });
    try {
        performance.dispatch('accept', new Uint8Array([144, 60, 100]), 'notes');
        performance.dispatch('advance');
        context.currentTime = 10.010;
        performance.dispatch('accept', new Uint8Array([128, 60, 0]), 'notes');
        context.currentTime = 10.130;
        performance.dispatch('advance');
        assert.equal(output.filter(e => e.port === 'note_start').length, 2);
        performance.stop();
        assert.deepEqual(performance.visibleNotes(), []);
        assert.deepEqual(output.at(-1), { silence: true });
        context.currentTime = 10.200;
        performance.dispatch('accept', new Uint8Array([251]), 'clock');
        context.currentTime = 10.325;
        performance.dispatch('advance');
        assert.equal(output.filter(e => e.port === 'note_start').length, 3);
    } finally { performance.dispose(); }
});

test('entry bend is scheduled before its note while a later live change retains its own time', () => {
    const { performance, context, output } = instrument();
    try {
        performance.dispatch('accept', new Uint8Array([224, 0, 0]), 'notes');
        performance.dispatch('accept', new Uint8Array([144, 60, 100]), 'notes');
        performance.dispatch('advance');
        assert.deepEqual(output.map(e => e.at), [10.040, 10.040]);
        assert.deepEqual(output[0].bend, [224, 0, 0]);
        context.currentTime = 10.130;
        performance.dispatch('accept', new Uint8Array([224, 127, 127]), 'notes');
        assert.ok(Math.abs(output.at(-2).at - 10.165) < 1e-9);
        assert.ok(Math.abs(output.at(-1).at - 10.170) < 1e-9);
    } finally { performance.dispose(); }
});

test('release levels follow attack, decay, and sustain including a zero-length gate', () => {
    const envelope = { attack: 0.1, decay: 0.2, sustain: 0.5 };
    assert.equal(envelopeLevel(0, envelope), 0);
    assert.equal(envelopeLevel(0.05, envelope), 0.5);
    assert.equal(envelopeLevel(0.1, envelope), 1);
    assert.equal(envelopeLevel(0.2, envelope), 0.75);
    assert.equal(envelopeLevel(1, envelope), 0.5);
});
