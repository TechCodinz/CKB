// Service layer background worker. Imports core/util.
import { formatId } from "../core/util";

export function runWorker(jobCount: number): string {
  return formatId("job", jobCount);
}
