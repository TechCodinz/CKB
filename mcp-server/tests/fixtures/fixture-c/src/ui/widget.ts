// UI layer. Legitimate imports: core/util and core/a.
import { formatId, clampScore } from "../core/util";
import { runPipeline } from "../core/a";

export function renderWidget(score: number): string {
  return formatId("widget", clampScore(score)) + ":" + runPipeline();
}
