/** Three soft chimes for a timer / reminder (WebAudio, no sound files). */
export function beep() {
  try {
    const ctx = new AudioContext();
    const t0 = ctx.currentTime + 0.02;
    for (let i = 0; i < 3; i++) {
      const o = ctx.createOscillator();
      const g = ctx.createGain();
      o.type = "sine";
      o.frequency.value = i === 2 ? 1175 : 880;
      const t = t0 + i * 0.28;
      g.gain.setValueAtTime(0, t);
      g.gain.linearRampToValueAtTime(0.25, t + 0.02);
      g.gain.exponentialRampToValueAtTime(0.001, t + 0.26);
      o.connect(g).connect(ctx.destination);
      o.start(t);
      o.stop(t + 0.28);
    }
    window.setTimeout(() => ctx.close(), 1400);
  } catch {
    /* no audio */
  }
}
