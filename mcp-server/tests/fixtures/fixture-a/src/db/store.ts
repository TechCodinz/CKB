// Data layer. Legitimate import of core/util.
import { formatId } from "../core/util";

export interface Record {
  id: string;
  value: number;
}

const rows: Record[] = [];

export function saveRecord(value: number): Record {
  const rec: Record = { id: formatId("rec", rows.length + 1), value };
  rows.push(rec);
  return rec;
}

export function readAll(): Record[] {
  return rows.slice();
}
