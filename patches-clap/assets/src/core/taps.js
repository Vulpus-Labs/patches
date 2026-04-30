export function tapsSignature(taps) {
  if (!taps) return "";
  return taps
    .map((t) => `${t.slot}:${t.name}:${(t.components ?? []).join(",")}`)
    .join("|");
}

export function cssEscape(s) {
  return String(s).replace(/(["\\])/g, "\\$1");
}

// Fold `~stereo_meter(foo)` halves (manifest names `foo/left` and
// `foo/right` at consecutive slots) into one paired entry labelled by
// stem (ADR 0059 §7). Mono taps stay as { stereo: false, tap }.
export function foldStereoPairs(sorted) {
  const out = [];
  let k = 0;
  while (k < sorted.length) {
    const cur = sorted[k];
    const comps = cur.components ?? [];
    const hasStereo = comps.includes("stereo_meter");
    const endsLeft = typeof cur.name === "string" && cur.name.endsWith("/left");
    const next = sorted[k + 1];
    if (hasStereo && endsLeft && next) {
      const nextComps = next.components ?? [];
      const endsRight = typeof next.name === "string" && next.name.endsWith("/right");
      const stem = cur.name.slice(0, -5);
      const nextStem = endsRight ? next.name.slice(0, -6) : null;
      if (nextComps.includes("stereo_meter") && nextStem === stem && next.slot === cur.slot + 1) {
        out.push({ stereo: true, stem, left: cur, right: next });
        k += 2;
        continue;
      }
    }
    out.push({ stereo: false, tap: cur });
    k += 1;
  }
  return out;
}
