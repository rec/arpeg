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

test('Motion expression updates immediately and restores known values at handoff', () => {
    const profile = readFileSync(new URL('../../../conformance/motion-expression.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        player.accept(0n, new Uint8Array([144, 60, 100]), 'both');
        assert.deepEqual(player.advance(0n), [new Uint8Array([144, 60, 100])]);
        assert.throws(() => player.control(0n, 'breath', '2'));
        assert.deepEqual(player.control(0n, 'breath', '1/2'), [new Uint8Array([176, 2, 64])]);
        assert.deepEqual(player.control(25_000n, 'bend', '-1'), [new Uint8Array([224, 0, 0])]);
        assert.deepEqual(player.control(50_000n, 'pressure', '1'), [new Uint8Array([208, 127])]);
        assert.deepEqual(player.accept(75_000n, new Uint8Array([176, 2, 127]), 'both'), []);
        assert.deepEqual(player.advance(125_000n), [
            new Uint8Array([128, 60, 0]), new Uint8Array([176, 2, 64]),
            new Uint8Array([224, 0, 0]), new Uint8Array([208, 127]), new Uint8Array([144, 60, 100]),
        ]);
    } finally {
        player.free();
    }
});

test('transposition controls and preset boundaries work through web bindings', () => {
    const profile = readFileSync(new URL('../../../conformance/transpose-fold.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        player.accept(0n, new Uint8Array([144, 127, 100]), 'both');
        assert.deepEqual(player.advance(0n), [new Uint8Array([144, 127, 100])]);
        assert.throws(() => player.control(50_000n, 'transposition', '1/2'));
        assert.deepEqual(player.control(50_000n, 'transposition', '-128'), []);
        assert.deepEqual(player.advance(125_000n), [new Uint8Array([128, 127, 0]), new Uint8Array([144, 11, 100])]);
        assert.deepEqual(player.stop(150_000n), [new Uint8Array([128, 11, 0])]);
    } finally {
        player.free();
    }
});

test('selection offset uses source ranks and configurable rests through web bindings', () => {
    const profile = readFileSync(new URL('../../../conformance/offset-rest.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        player.accept(0n, new Uint8Array([144, 60, 100]), 'both');
        player.accept(0n, new Uint8Array([144, 64, 90]), 'both');
        assert.deepEqual(player.advance(0n), [new Uint8Array([144, 64, 90])]);
        assert.throws(() => player.control(50_000n, 'selection_offset', '1/2'));
        assert.deepEqual(player.control(50_000n, 'selection_offset', '-1'), []);
        assert.deepEqual(player.advance(125_000n), [new Uint8Array([128, 64, 0]), new Uint8Array([144, 60, 100])]);
        player.control(150_000n, 'selection_offset', '2');
        assert.deepEqual(player.advance(250_000n), [new Uint8Array([128, 60, 0])]);
        assert.equal(player.take_events().events.at(-1).port, 'rest');
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

test('rhythm cycle notifications keep their exact phase through rests and seeks', () => {
    const profile = readFileSync(new URL('../../../conformance/cycle-rests.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        player.control(0n, 'density', '0');
        assert.deepEqual(player.advance(0n), []);
        assert.deepEqual(player.take_events().events.map(e => [e.port, e.index]), [['cycle', 0n], ['step', 0n], ['rest', 0n]]);
        player.advance(375_000n);
        assert.deepEqual(player.take_events().events.filter(e => e.port === 'cycle'), [
            { at: '3/4', port: 'cycle', index: 3n, revision: 0n },
        ]);
        player.accept(400_000n, new Uint8Array([242, 6, 0]), 'both');
        player.advance(400_000n);
        assert.deepEqual(player.take_events().events.map(e => [e.port, e.index]), [['cycle', 6n], ['step', 6n], ['rest', 6n]]);
    } finally {
        player.free();
    }
});

test('capture readiness reports publication before the step and omits empty undo', () => {
    const profile = readFileSync(new URL('../../../conformance/phrase-wind.toml', import.meta.url), 'utf8');
    const player = new MidiPlayer(profile, 'internal', 120, 500_000n);
    try {
        player.capture(0n, 'record');
        player.accept(0n, new Uint8Array([144, 60, 100]), 'both');
        player.capture(10_000n, 'commit');
        assert.deepEqual(player.take_events().events.map(e => e.port), ['step', 'rest']);
        player.advance(125_000n);
        assert.deepEqual(player.take_events(), { events: [
            { at: '1/4', port: 'capture_ready', index: 1n, revision: 1n },
            { at: '1/4', port: 'step', index: 1n, revision: 1n },
            { at: '1/4', port: 'hit', index: 1n, revision: 1n },
        ], exhausted: false });
        player.advance(250_000n);
        assert.deepEqual(player.take_events().events.map(e => e.port), ['step', 'hit']);
        player.capture(260_000n, 'undo');
        player.advance(375_000n);
        assert.deepEqual(player.take_events().events.map(e => [e.port, e.revision]), [['step', 2n], ['rest', 2n]]);
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
