// Shared utility module. Ground truth: imported directly by 5 files
// (ui/panel.ts, ui/widget.ts, service/api.ts, service/worker.ts, db/store.ts).
export const UTIL_VERSION = "1.0.0";

export function formatId(prefix: string, n: number): string {
  return prefix + "-" + n.toString().padStart(4, "0");
}

export function clampScore(score: number): number {
  if (score < 0) return 0;
  if (score > 100) return 100;
  return score;
}
