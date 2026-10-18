// UI layer. DELIBERATE LAYER VIOLATION: ui imports db directly,
// skipping the service layer. Also imports core/util (legitimate).
import { readAll } from "../db/store";
import { formatId } from "../core/util";

export function renderPanel(): string {
  const records = readAll();
  return "panel[" + formatId("panel", records.length) + "]:" + records.length;
}
