/**
 * The sound a finished batch makes: three rising 100 ms tones at 500, 1200,
 * and 2000 Hz.
 *
 * The same tones the original script played, but through Web Audio rather
 * than shelling out to SoX's `play -t alsa`, which existed only on Linux and
 * printed an error everywhere else.
 */

const TONES = [500, 1200, 2000];
const TONE_SECS = 0.1;
/** Quiet: this plays over whatever else the machine is doing. */
const VOLUME = 0.08;

export function playChime() {
  try {
    const context = new AudioContext();
    const start = context.currentTime;
    TONES.forEach((frequency, i) => {
      const at = start + i * TONE_SECS;
      const oscillator = context.createOscillator();
      const gain = context.createGain();
      oscillator.type = 'sine';
      oscillator.frequency.value = frequency;
      // A short ramp at each end, or every tone starts and stops with a click.
      gain.gain.setValueAtTime(0, at);
      gain.gain.linearRampToValueAtTime(VOLUME, at + 0.01);
      gain.gain.linearRampToValueAtTime(0, at + TONE_SECS - 0.005);
      oscillator.connect(gain).connect(context.destination);
      oscillator.start(at);
      oscillator.stop(at + TONE_SECS);
    });
    setTimeout(() => void context.close(), (TONES.length * TONE_SECS + 0.2) * 1000);
  } catch {
    /* no audio device; the toast still says the batch finished */
  }
}
