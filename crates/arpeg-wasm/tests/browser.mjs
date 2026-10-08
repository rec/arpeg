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
