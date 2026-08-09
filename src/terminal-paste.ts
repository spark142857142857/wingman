export type TerminalPasteV1 =
  | { kind: "single-line"; data: string }
  | { kind: "line-breaking"; data: string };

export function classifyTerminalPaste(data: string): TerminalPasteV1 {
  return {
    kind: /[\r\n]/.test(data) ? "line-breaking" : "single-line",
    data,
  };
}
