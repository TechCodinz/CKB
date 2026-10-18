// FIXTURE-B ADDITION: new file, imports core/util.
// Ground truth: util.ts gains a 6th direct importer; +1 file, +1 import edge.
import { clampScore, formatId } from "../core/util";

export function buildReport(score: number): string {
  return formatId("report", clampScore(score));
}
