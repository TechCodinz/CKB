// Part A of the deliberate circular pair: a.ts -> b.ts -> a.ts.
import { stepB } from "./b";

export function stepA(depth: number): string {
  if (depth <= 0) return "a-done";
  return "a>" + stepB(depth - 1);
}

export function runPipeline(): string {
  return stepA(3);
}
