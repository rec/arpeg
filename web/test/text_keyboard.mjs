import assert from 'node:assert/strict';
import { test } from 'node:test';
import { TextKeyboard } from '../text_keyboard.mjs';

test('keyboard repeats are ignored and equal characters keep distinct input identities', () => {
    const presses = [];
    const releases = [];
    const player = {
        press(at, identity, rank, token) { presses.push([at, identity, rank, token]); return [token]; },
        release(at, identity) { releases.push([at, identity]); return []; },
        advance() { return [1, 0]; },
        stop() { return []; },
    };
    const keyboard = new TextKeyboard(player);
    assert.deepEqual(keyboard.press(0n, 'KeyA', 'λ'), ['λ']);
    assert.deepEqual(keyboard.press(1n, 'KeyA', 'λ'), []);
    assert.deepEqual(keyboard.press(2n, 'KeyB', 'λ'), ['λ']);
    assert.deepEqual(presses, [[0n, 0, 0, 0], [2n, 1, 1, 1]]);
    keyboard.release(3n, 'KeyA');
    keyboard.release(3n, 'KeyA');
    assert.deepEqual(releases, [[3n, 0]]);
    assert.deepEqual(keyboard.advance(4n), ['λ', 'λ']);
    keyboard.stop(5n);
    assert.equal(keyboard.held.size, 0);
});
