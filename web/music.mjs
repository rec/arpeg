export const audioDelay = 0.040;

export const orders = {
    ascending: 'Up',
    descending: 'Down',
    alternating: 'Up & down',
    inside_out: 'Inside out',
    outside_in: 'Outside in',
    played: 'As played',
    shuffle: 'Shuffle',
};

export const typingKeys = new Map(
    ['KeyA', 'KeyW', 'KeyS', 'KeyE', 'KeyD', 'KeyF', 'KeyT',
        'KeyG', 'KeyY', 'KeyH', 'KeyU', 'KeyJ', 'KeyK']
        .map((code, index) => [code, 60 + index]),
);

export function profileFor({ order, latch, step, gate, transpose }) {
    return `name = "browser"
title = "Browser performance"
[body]
bank = { kind = "${latch ? 'latched' : 'held'}"${latch ? ', update = "replace"' : ''} }
selection = { kind = "${order}" }
rhythm = { kind = "grid", step = "${step} beat" }
gate = "${gate}/100"
transposition = { semitones = ${transpose}, boundary = "fold" }
seed = 42
`;
}

export function noteName(key) {
    return `${['C', 'C♯', 'D', 'D♯', 'E', 'F', 'F♯', 'G', 'G♯', 'A', 'A♯', 'B'][key % 12]}${Math.floor(key / 12) - 1}`;
}

export function beatNumber(beat) {
    const [numerator, denominator = '1'] = beat.split('/');
    return Number(numerator) / Number(denominator);
}

// Keep input at real time. Only the audio presentation is delayed.
export function audioTime(noteBeat, currentBeat, bpm, now) {
    return Math.max(now, now + audioDelay -
        (beatNumber(currentBeat) - beatNumber(noteBeat)) * 60 / bpm);
}

export class HeldKeys {
    owners = new Map();

    press(owner, key) {
        if (this.owners.has(owner)) return false;
        const alreadyHeld = [...this.owners.values()].includes(key);
        this.owners.set(owner, key);
        return !alreadyHeld;
    }

    release(owner) {
        const key = this.owners.get(owner);
        this.owners.delete(owner);
        return key !== undefined && ![...this.owners.values()].includes(key) ? key : null;
    }

    get notes() {
        return [...new Set(this.owners.values())].sort((a, b) => a - b);
    }
}
