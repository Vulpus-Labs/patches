// First rising zero-cross in s[1..end). Schmitt-armed: requires the
// signal to have dipped below -eps before the upward zero-cross to
// suppress retrigger on noise / harmonic ripple. eps = 5% of the
// window peak amplitude. Returns null if none. Ticket 0754.
export function findFirstRisingCross(s, end) {
  let peak = 0;
  for (let i = 0; i < end; i++) {
    const a = s[i] < 0 ? -s[i] : s[i];
    if (a > peak) peak = a;
  }
  const eps = peak * 0.05;
  let armed = false;
  for (let j = 1; j < end; j++) {
    if (s[j] < -eps) armed = true;
    else if (armed && s[j - 1] < 0 && s[j] >= 0) return j;
  }
  return null;
}
