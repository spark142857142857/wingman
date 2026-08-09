import { classifyTerminalPaste } from "./terminal-paste.ts";

function assertEqual(label: string, actual: unknown, expected: unknown) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(
      `${label}\nexpected: ${JSON.stringify(expected)}\nactual:   ${JSON.stringify(actual)}`,
    );
  }
}

assertEqual(
  "single-line paste remains insert-only",
  classifyTerminalPaste("grep TODO src\\main.ts"),
  { kind: "single-line", data: "grep TODO src\\main.ts" },
);

for (const value of ["one\ntwo", "one\rtwo", "one\r\ntwo", "one\n"]) {
  assertEqual(
    `line-breaking paste preserves exact bytes: ${JSON.stringify(value)}`,
    classifyTerminalPaste(value),
    { kind: "line-breaking", data: value },
  );
}

console.log("Terminal paste tests passed.");
