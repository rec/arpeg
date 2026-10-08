export function envelopeLevel(elapsed, { attack, decay, sustain }) {
    if (elapsed <= 0) return 0;
    if (elapsed < attack) return elapsed / attack;
    if (elapsed < attack + decay) return 1 - (1 - sustain) * (elapsed - attack) / decay;
    return sustain;
}

export class Synth {
    voices = new Map();
    bend = 0;

    constructor(context) {
        this.context = context;
        this.master = context.createGain();
        this.master.gain.value = 0.2;
        this.master.connect(context.destination);
    }

    note(event, at, settings) {
        if (event.port === 'note_start') {
            const oscillator = this.context.createOscillator();
            const gain = this.context.createGain();
            const envelope = { ...settings };
            const peak = event.velocity / 127 * 0.5;
            oscillator.type = settings.waveform;
            oscillator.frequency.value = 440 * 2 ** ((event.key - 69) / 12);
            oscillator.detune.setValueAtTime(this.bend * 200, at);
            gain.gain.setValueAtTime(0, at);
            gain.gain.linearRampToValueAtTime(peak, at + envelope.attack);
            gain.gain.linearRampToValueAtTime(peak * envelope.sustain, at + envelope.attack + envelope.decay);
            oscillator.connect(gain);
            gain.connect(this.master);
            const voice = { oscillator, gain, envelope, peak, at, ended: false };
            this.voices.set(event.occurrence, voice);
            oscillator.onended = () => {
                oscillator.disconnect();
                gain.disconnect();
                if (this.voices.get(event.occurrence) === voice) {
                    this.voices.delete(event.occurrence);
                }
            };
            oscillator.start(at);
        } else {
            const voice = this.voices.get(event.occurrence);
            if (!voice || voice.ended) return;
            this.release(voice, at);
        }
    }

    release(voice, at) {
        const { gain, envelope, peak } = voice;
        const elapsed = Math.max(0, at - voice.at);
        const level = peak * envelopeLevel(elapsed, envelope);
        // Rebuild the part of ADS that precedes release, including short gates.
        gain.gain.cancelScheduledValues(voice.at);
        gain.gain.setValueAtTime(0, voice.at);
        if (elapsed >= envelope.attack) {
            gain.gain.linearRampToValueAtTime(peak, voice.at + envelope.attack);
        }
        if (elapsed >= envelope.attack + envelope.decay) {
            gain.gain.linearRampToValueAtTime(peak * envelope.sustain, voice.at + envelope.attack + envelope.decay);
            gain.gain.setValueAtTime(level, at);
        } else {
            gain.gain.linearRampToValueAtTime(level, at);
        }
        gain.gain.linearRampToValueAtTime(0, at + envelope.release);
        voice.oscillator.stop(at + envelope.release);
        voice.ended = true;
    }

    pitchBend(data, at) {
        const value = data[1] + data[2] * 128;
        this.bend = (value - 8192) / (value < 8192 ? 8192 : 8191);
        for (const voice of this.voices.values()) {
            if (!voice.ended) voice.oscillator.detune.setValueAtTime(this.bend * 200, at);
        }
    }

    silence() {
        const now = this.context.currentTime;
        for (const voice of this.voices.values()) {
            voice.gain.gain.cancelScheduledValues(now);
            if (voice.at >= now) {
                voice.gain.gain.setValueAtTime(0, now);
                voice.oscillator.stop(now);
            } else {
                voice.gain.gain.setTargetAtTime(0, now, 0.003);
                voice.oscillator.stop(now + 0.015);
            }
            voice.ended = true;
        }
    }
}
