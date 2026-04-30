export const pad2 = (n) => (n < 10 ? `0${n}` : `${n}`);

export function nowHms(date = new Date()) {
  return `${pad2(date.getUTCHours())}:${pad2(date.getUTCMinutes())}:${pad2(date.getUTCSeconds())}`;
}

// Reconcile a parallel HH:MM:SS array with a new lines array. The Rust
// side does not stamp entries; we stamp on first sight so the UI shows
// when the entry *arrived*. Diff rules:
//   - new shorter than old → assume eviction from front, drop matching prefix
//   - tail extension → stamp the new tail with `nowStamp`
// Returns the reconciled stamp array. Pure: takes the wall-clock stamp
// as a parameter so tests can pin it.
export function computeLogStamps(prevStamps, prevLen, nextLen, nowStamp) {
  let stamps = prevStamps;
  if (nextLen < prevLen) {
    stamps = stamps.slice(prevLen - nextLen);
  }
  if (nextLen > stamps.length) {
    stamps = stamps.slice();
    while (stamps.length < nextLen) stamps.push(nowStamp);
  }
  return stamps;
}
