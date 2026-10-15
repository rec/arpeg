// Host strings stay outside the engine. Handles, ranks, and input identities
// are separate numbers; characters are never encoded as MIDI or pitch values.
export class TextKeyboard {
    constructor(player) {
        this.player = player;
        this.held = new Map();
        this.tokens = new Map();
        this.next = 0;
    }

    press(at, physicalKey, text) {
        if (this.held.has(physicalKey)) return [];
        const identity = this.next++;
        this.held.set(physicalKey, identity);
        this.tokens.set(identity, text);
        return this.decode(this.player.press(at, identity, identity, identity));
    }

    release(at, physicalKey) {
        const identity = this.held.get(physicalKey);
        if (identity === undefined) return [];
        this.held.delete(physicalKey);
        return this.decode(this.player.release(at, identity));
    }

    advance(at) { return this.decode(this.player.advance(at)); }

    stop(at) {
        const output = this.decode(this.player.stop(at));
        this.held.clear();
        this.tokens.clear();
        return output;
    }

    decode(handles) { return handles.map(handle => this.tokens.get(handle)); }
}
