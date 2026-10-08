import assert from 'node:assert/strict';
import test from 'node:test';
import { Synth } from '../synth.mjs';

const settings = { waveform: 'triangle', attack: 0.01, decay: 0.1, sustain: 0.6, release: 0.12 };

function audio() {
    const oscillators = [];
    const parameter = () => ({
        value: 0,
        setValueAtTime() {}, linearRampToValueAtTime() {},
        cancelScheduledValues() {}, setTargetAtTime() {},
    });
    const context = {
        currentTime: 1,
        destination: {},
        createGain: () => ({ gain: parameter(), connect() {}, disconnect() {} }),
        createOscillator: () => {
            const oscillator = {
                frequency: parameter(), detune: parameter(),
                connect() {}, disconnect() {},
                start(at) { this.startTime = at; },
                stop(at) { this.stopTime = at; },
            };
            oscillators.push(oscillator);
            return oscillator;
        },
    };
    return { context, oscillators, synth: new Synth(context) };
}

test('Stop cancels a queued voice before it can attack', () => {
    const { synth, oscillators } = audio();
    synth.note({ port: 'note_start', occurrence: 0n, key: 60, velocity: 100 }, 1.040, settings);
    synth.silence();
    assert.ok(oscillators[0].stopTime < oscillators[0].startTime);
});

test('a retired voice cannot remove a new player’s occurrence with the same ID', () => {
    const { synth, context, oscillators } = audio();
    const event = { port: 'note_start', occurrence: 0n, key: 60, velocity: 100 };
    synth.note(event, 1.040, settings);
    synth.silence();
    synth.note(event, 1.050, settings);
    oscillators[0].onended();
    context.currentTime = 1.100;
    synth.note({ ...event, port: 'note_end' }, 1.150, settings);
    assert.ok(Math.abs(oscillators[1].stopTime - 1.270) < 1e-9);
});
