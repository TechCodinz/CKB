// Entry point: ui + service layers only.
import { renderPanel } from "./ui/panel";
import { renderWidget } from "./ui/widget";
import { handleRequest } from "./service/api";

export function bootstrap(): string {
  return [renderPanel(), renderWidget(42), handleRequest(7)].join(" | ");
}
