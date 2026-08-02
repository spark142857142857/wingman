import { isPasteShortcut } from "./terminal-shortcuts.ts";

function assertEqual(label: string, actual: unknown, expected: unknown) {
  if (actual !== expected) {
    throw new Error(`${label}: expected ${JSON.stringify(expected)}, got ${JSON.stringify(actual)}`);
  }
}

const keyboardEvent = (overrides: Partial<KeyboardEvent>): Pick<
  KeyboardEvent,
  "altKey" | "ctrlKey" | "key" | "metaKey"
> => ({
  altKey: false,
  ctrlKey: false,
  key: "",
  metaKey: false,
  ...overrides,
});

assertEqual(
  "Ctrl+V uses Wingman clipboard handling",
  isPasteShortcut(keyboardEvent({ ctrlKey: true, key: "v" })),
  true,
);
assertEqual(
  "Ctrl+Shift+V also uses Wingman clipboard handling",
  isPasteShortcut(keyboardEvent({ ctrlKey: true, key: "V" })),
  true,
);
assertEqual(
  "Alt+Ctrl+V remains available to the shell",
  isPasteShortcut(keyboardEvent({ altKey: true, ctrlKey: true, key: "v" })),
  false,
);
assertEqual(
  "ordinary input is not a paste shortcut",
  isPasteShortcut(keyboardEvent({ key: "v" })),
  false,
);

console.log("Terminal shortcut tests passed.");
