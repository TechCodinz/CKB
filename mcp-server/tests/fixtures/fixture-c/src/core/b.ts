// Part B of the deliberate circular pair: b.ts imports a.ts (cycle).
import { stepA } from "./a";

export function stepB(depth: number): string {
  if (depth <= 0) return "b-done";
  return "b>" + stepA(depth - 1);
}
