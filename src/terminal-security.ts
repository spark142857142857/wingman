import type { ILinkHandler } from "@xterm/xterm";

// Terminal output is untrusted. P0 has no external-link feature, so both OSC 8
// hyperlinks and auto-detected URLs must remain inert until a separately tested
// OS-browser allowlist is introduced.
export const blockedTerminalLinkHandler: ILinkHandler = {
  activate: () => undefined,
};
