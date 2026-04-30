// Project a diagnostic record onto the strings the diagnostics list
// renderer needs: severity class suffix, message, joined meta line.
// `metaText` is null when there are no meta parts (location/label/code),
// signalling the renderer to skip the meta row entirely.
export function formatDiagnostic(d) {
  const metaParts = [];
  if (d.location) metaParts.push(d.location);
  if (d.label) metaParts.push(d.label);
  if (d.code) metaParts.push(`[${d.code}]`);
  return {
    sevClass: `sev-${d.severity ?? "note"}`,
    message: d.message ?? "",
    metaText: metaParts.length > 0 ? metaParts.join("  ") : null,
  };
}
