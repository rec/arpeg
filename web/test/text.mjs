import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import { test } from 'node:test';
import init, { TokenPlayer } from '../../dist/wasm/arpeg_wasm.js';
import { TextKeyboard } from '../text_keyboard.mjs';

await init({ module_or_path: await readFile(new URL('../../dist/wasm/arpeg_wasm_bg.wasm', import.meta.url)) });

test('the actual WASM engine arpeggiates text in entry order without MIDI messages', () => {
    const keyboard = new TextKeyboard(new TokenPlayer('ascending', false, '1/4', '4/5', '1/2', ['1'], ['1']));
    try {
        keyboard.press(0n, 'KeyZ', 'z');
        keyboard.press(0n, 'KeyA', 'a');
        keyboard.press(0n, 'KeyL', 'λ');
        assert.deepEqual(keyboard.advance(0n), ['z']);
        assert.deepEqual(keyboard.advance(125000n), ['a']);
        assert.deepEqual(keyboard.advance(250000n), ['λ']);
        keyboard.release(300000n, 'KeyZ');
        assert.deepEqual(keyboard.advance(375000n), ['a']);
        assert.deepEqual(keyboard.stop(400000n), []);
        assert.deepEqual(keyboard.advance(500000n), []);
    } finally { keyboard.player.free(); }
});

test('latched Unicode text persists through key releases and is replaced by a new group', () => {
    const keyboard = new TextKeyboard(new TokenPlayer('ascending', true, '1/4', '4/5', '1/2', ['1'], ['1']));
    try {
        keyboard.press(0n, 'KeyL', 'λ');
        assert.deepEqual(keyboard.advance(0n), ['λ']);
        keyboard.release(50000n, 'KeyL');
        assert.deepEqual(keyboard.advance(125000n), ['λ']);
        keyboard.press(150000n, 'KeyX', '😀');
        assert.deepEqual(keyboard.advance(250000n), ['😀']);
        keyboard.stop(300000n);
    } finally { keyboard.player.free(); }
});

test('preset swing delays the second character while keeping the next pair on time', () => {
    const keyboard = new TextKeyboard(new TokenPlayer('ascending', false, '1/4', '4/5', '2/3', ['1'], ['1']));
    try {
        keyboard.press(0n, 'KeyA', 'a');
        keyboard.press(0n, 'KeyB', 'b');
        assert.deepEqual(keyboard.advance(0n), ['a']);
        assert.deepEqual(keyboard.advance(125000n), []);
        assert.deepEqual(keyboard.advance(166667n), ['b']);
        assert.deepEqual(keyboard.advance(250000n), ['a']);
        keyboard.stop(300000n);
    } finally { keyboard.player.free(); }
});

test('a muted opportunity does not consume the next text selection', () => {
    const keyboard = new TextKeyboard(new TokenPlayer('ascending', false, '1/4', '4/5', '1/2', ['1', '0', '1/2'], ['1', '1/2']));
    try {
        keyboard.press(0n, 'KeyA', 'a');
        keyboard.press(0n, 'KeyB', 'b');
        assert.deepEqual(keyboard.advance(0n), ['a']);
        assert.deepEqual(keyboard.advance(125000n), []);
        assert.deepEqual(keyboard.advance(250000n), ['b']);
        keyboard.stop(300000n);
    } finally { keyboard.player.free(); }
});
