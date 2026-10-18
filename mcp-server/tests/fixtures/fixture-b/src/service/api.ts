// Service layer. Legitimate imports: core, db (service -> db is the
// sanctioned path), and core/util.
import { stepB } from "../core/b";
import { clampScore } from "../core/util";
import { readAll, saveRecord } from "../db/store";

export function handleRequest(score: number): string {
  saveRecord(clampScore(score));
  return stepB(2) + ":" + readAll().length;
}
