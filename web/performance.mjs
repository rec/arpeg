import { audioDelay, audioTime } from './music.mjs';

export class Performance {
    visualNotes = [];
    sounding = new Map();

    constructor(Player, profile, bpm, context, synth, settings) {
        this.player = new Player(profile, 'internal', bpm, 500_000n);
        this.bpm = bpm;
        this.context = context;
        this.synth = synth;
        this.settings = settings;
        this.dispatch('stop');
    }

    dispatch(method, ...args) {
        const now = this.context.currentTime;
        const messages = this.player[method](BigInt(Math.round(now * 1_000_000)), ...args);
        const batch = this.player.take_events();
        let index = 0;
        for (const data of messages) {
            const kind = data[0] & 0xf0;
            if (kind === 0x90 || kind === 0x80) {
                const event = batch.notes[index++];
                const at = audioTime(event.at, this.player.beat, this.bpm, now);
                this.synth.note(event, at, this.settings());
                this.visualNotes.push({ event, at });
            } else if (kind === 0xe0) {
                // Entry expression precedes its attack; a live change follows due notes.
                const next = batch.notes[index];
                const at = next?.port === 'note_start'
                    ? audioTime(next.at, this.player.beat, this.bpm, now) : now + audioDelay;
                this.synth.pitchBend(data, at);
            }
        }
        if (batch.exhausted) throw new Error('The browser fell behind. Playback has been stopped; press Start to resume.');
    }

    tempo(bpm) {
        // Finish old-tempo events before changing the beat-to-seconds conversion.
        this.dispatch('advance');
        this.dispatch('set_tempo', bpm);
        this.bpm = bpm;
    }

    stop() {
        this.dispatch('stop');
        this.synth.silence();
        this.visualNotes = [];
        this.sounding.clear();
    }

    visibleNotes() {
        while (this.visualNotes.length && this.visualNotes[0].at <= this.context.currentTime) {
            const { event } = this.visualNotes.shift();
            if (event.port === 'note_start') this.sounding.set(event.occurrence, event.key);
            else this.sounding.delete(event.occurrence);
        }
        return [...this.sounding.values()];
    }

    dispose() {
        this.stop();
        this.player.free();
    }
}
