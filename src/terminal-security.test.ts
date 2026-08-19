import assert from "node:assert/strict";
import { blockedTerminalLinkHandler } from "./terminal-security";

let opened = false;
Object.defineProperty(globalThis, "window", {
  configurable: true,
  value: {
    open: () => {
      opened = true;
      throw new Error("terminal output attempted to open a window");
    },
  },
});

blockedTerminalLinkHandler.activate({} as MouseEvent, "https://example.invalid", {
  start: { x: 1, y: 1 },
  end: { x: 1, y: 1 },
});
assert.equal(opened, false, "terminal links must not call window.open");

console.log("terminal link security tests passed");
