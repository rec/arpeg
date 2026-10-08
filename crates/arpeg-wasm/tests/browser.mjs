import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { initSync, MidiPlayer } from '../../../dist/wasm/arpeg_wasm.js';

test('generated web bindings load and exchange MIDI messages', () => {
    initSync({
        module: readFileSync(new URL('../../../dist/wasm/arpeg_wasm_bg.wasm', import.meta.url)),
    });
    const profile = readFileSync(new URL('../../../conformance/up.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        assert.deepEqual(player.accept(0n, new Uint8Array([0x90, 60, 100]), 'both'), []);
        assert.deepEqual(player.advance(0n), [new Uint8Array([0x90, 60, 100])]);
        assert.equal(player.beat, '0');
        assert.equal(player.active, true);
        assert.deepEqual(player.stop(50_000n), [new Uint8Array([0x80, 60, 0])]);
        assert.equal(player.active, false);
        assert.throws(() => player.accept(50_000n, new Uint8Array(), 'both'));
    } finally {
        player.free();
    }
});

test('Motion ports return exact event coordinates and accept step-boundary controls', () => {
    const profile = readFileSync(new URL('../../../conformance/motion-ports.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        player.accept(0n, new Uint8Array([0x90, 60, 100]), 'both');
        assert.deepEqual(player.control(0n, 'gate', '1/5'), []);
        assert.deepEqual(player.advance(0n), [new Uint8Array([0x90, 60, 100])]);
        assert.deepEqual(player.take_events(), { events: [
            { at: '0', port: 'step', index: 0n, revision: 1n },
            { at: '0', port: 'hit', index: 0n, revision: 1n },
        ], exhausted: false });
        assert.throws(() => player.control(10_000n, 'density', '2'));
        assert.throws(() => player.control(10_000n, 'other', '1'));
        assert.throws(() => player.control(10_000n, 'gate', '0.5'));
        assert.deepEqual(player.advance(25_000n), [new Uint8Array([0x80, 60, 0])]);
        assert.deepEqual(player.control(50_000n, 'density', '0'), []);
        assert.deepEqual(player.advance(125_000n), []);
        assert.deepEqual(player.take_events(), { events: [
            { at: '1/4', port: 'step', index: 1n, revision: 1n },
            { at: '1/4', port: 'rest', index: 1n, revision: 1n },
        ], exhausted: false });
    } finally {
        player.free();
    }
});

test('phrase controls publish committed takes and preserve sounding output', () => {
    const profile = readFileSync(new URL('../../../conformance/phrase-wind.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        assert.throws(() => player.capture(0n, 'commit'));
        assert.deepEqual(player.capture(0n, 'record'), []);
        assert.deepEqual(player.capture_state, [true, 0, 0]);
        player.accept(0n, new Uint8Array([0x90, 60, 100]), 'both');
        assert.deepEqual(player.capture(10_000n, 'commit'), []);
        assert.deepEqual(player.capture_state, [false, 0, 0]);
        assert.deepEqual(player.advance(125_000n), [new Uint8Array([0x90, 60, 100])]);
        assert.deepEqual(player.capture_state, [false, 1, 1]);
        player.capture(150_000n, 'record');
        player.accept(160_000n, new Uint8Array([0x90, 64, 90]), 'both');
        player.capture(175_000n, 'overdub');
        assert.deepEqual(player.advance(250_000n), [new Uint8Array([0x80, 60, 0]), new Uint8Array([0x90, 60, 100])]);
        assert.deepEqual(player.capture_state, [false, 2, 2]);
        assert.deepEqual(player.capture(260_000n, 'undo'), []);
        assert.deepEqual(player.advance(375_000n), [new Uint8Array([0x80, 60, 0]), new Uint8Array([0x90, 60, 100])]);
        assert.deepEqual(player.capture_state, [false, 1, 3]);
        assert.deepEqual(player.clear(380_000n), [new Uint8Array([0x80, 60, 0])]);
        assert.deepEqual(player.capture_state, [false, 0, 4]);
    } finally {
        player.free();
    }
});
