export type PasteShortcutEvent = Pick<
  KeyboardEvent,
  "altKey" | "ctrlKey" | "key" | "metaKey"
>;

export function isPasteShortcut(event: PasteShortcutEvent): boolean {
  return event.ctrlKey
    && !event.altKey
    && !event.metaKey
    && event.key.toLowerCase() === "v";
}
