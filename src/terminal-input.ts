export type TerminalInputAction =
  | { type: "write"; data: string }
  | { type: "submit"; line: string; reliable: boolean };

const ESC = "\u001b";

export class TerminalInputParser {
  private line = "";
  private cursor = 0;
  private escapeSequence = "";
  private reliable = true;
  private ignoreNextLineFeed = false;
  private history: string[] = [];
  private historyIndex: number | null = null;
  private historyDraft = "";

  reset() {
    this.line = "";
    this.cursor = 0;
    this.escapeSequence = "";
    this.reliable = true;
    this.ignoreNextLineFeed = false;
    this.history = [];
    this.historyIndex = null;
    this.historyDraft = "";
  }

  commitSubmittedLine(actualLine: string | null) {
    if (actualLine === null) {
      this.history = [];
      return;
    }
    if (actualLine.trim()) {
      this.history.push(actualLine);
      if (this.history.length > 100) this.history.shift();
    }
  }

  consume(data: string): TerminalInputAction[] {
    const actions: TerminalInputAction[] = [];

    for (const ch of data) {
      if (ch === "\n" && this.ignoreNextLineFeed) {
        this.ignoreNextLineFeed = false;
        continue;
      }
      this.ignoreNextLineFeed = false;

      if (this.escapeSequence) {
        this.escapeSequence += ch;
        if (this.escapeSequenceComplete()) {
          const sequence = this.escapeSequence;
          this.escapeSequence = "";
          const isTerminalBoundary =
            sequence === `${ESC}[I` ||
            sequence === `${ESC}[O` ||
            sequence === `${ESC}[200~` ||
            sequence === `${ESC}[201~`;
          if (!isTerminalBoundary) {
            if (!this.applyEditingSequence(sequence)) this.reliable = false;
            this.pushWrite(actions, sequence);
          }
        }
        continue;
      }

      if (ch === ESC) {
        this.escapeSequence = ESC;
        continue;
      }

      if (ch === "\r" || ch === "\n") {
        actions.push({ type: "submit", line: this.line, reliable: this.reliable });
        this.line = "";
        this.cursor = 0;
        this.reliable = true;
        this.ignoreNextLineFeed = ch === "\r";
        this.historyIndex = null;
        this.historyDraft = "";
        continue;
      }

      if (ch === "\u007f" || ch === "\b") {
        if (this.cursor > 0) {
          this.line = this.line.slice(0, this.cursor - 1) + this.line.slice(this.cursor);
          this.cursor--;
        }
        this.pushWrite(actions, ch);
        continue;
      }

      if (ch === "\u0003") {
        this.line = "";
        this.cursor = 0;
        this.reliable = true;
        this.historyIndex = null;
        this.historyDraft = "";
        this.pushWrite(actions, ch);
        continue;
      }

      if (this.applyControlCharacter(ch)) {
        this.pushWrite(actions, ch);
        continue;
      }

      if (ch >= " " && ch !== "\u007f") {
        this.line = this.line.slice(0, this.cursor) + ch + this.line.slice(this.cursor);
        this.cursor++;
      } else if (ch === "\t") {
        // Completion can replace arbitrary text, so the shell becomes the source
        // of truth until the next submitted command.
        this.reliable = false;
      }
      this.pushWrite(actions, ch);
    }

    return actions;
  }

  private escapeSequenceComplete() {
    if (this.escapeSequence.length < 2) return false;

    const introducer = this.escapeSequence[1];
    const last = this.escapeSequence.at(-1)!;

    if (introducer === "[") {
      if (this.escapeSequence.length < 3) return false;
      const code = last.charCodeAt(0);
      return code >= 0x40 && code <= 0x7e;
    }

    if (introducer === "O") return this.escapeSequence.length >= 3;

    if (introducer === "]") {
      return last === "\u0007" || this.escapeSequence.endsWith(`${ESC}\\`);
    }

    return true;
  }

  private pushWrite(actions: TerminalInputAction[], data: string) {
    const previous = actions.at(-1);
    if (previous?.type === "write") {
      previous.data += data;
    } else {
      actions.push({ type: "write", data });
    }
  }

  private applyEditingSequence(sequence: string): boolean {
    switch (sequence) {
      case `${ESC}[D`:
        this.cursor = Math.max(0, this.cursor - 1);
        return true;
      case `${ESC}[C`:
        this.cursor = Math.min(this.line.length, this.cursor + 1);
        return true;
      case `${ESC}[H`:
      case `${ESC}[1~`:
        this.cursor = 0;
        return true;
      case `${ESC}[F`:
      case `${ESC}[4~`:
        this.cursor = this.line.length;
        return true;
      case `${ESC}[3~`:
        if (this.cursor < this.line.length) {
          this.line = this.line.slice(0, this.cursor) + this.line.slice(this.cursor + 1);
        }
        return true;
      case `${ESC}[A`:
        return this.recallPreviousHistory();
      case `${ESC}[B`:
        return this.recallNextHistory();
      default:
        return false;
    }
  }

  private applyControlCharacter(ch: string): boolean {
    switch (ch) {
      case "\u0001": // Ctrl+A
        this.cursor = 0;
        return true;
      case "\u0005": // Ctrl+E
        this.cursor = this.line.length;
        return true;
      case "\u0015": // Ctrl+U
        this.line = this.line.slice(this.cursor);
        this.cursor = 0;
        return true;
      case "\u000b": // Ctrl+K
        this.line = this.line.slice(0, this.cursor);
        return true;
      case "\u0017": { // Ctrl+W
        const before = this.line.slice(0, this.cursor);
        const start = before.search(/\S+\s*$/);
        if (start >= 0) {
          this.line = this.line.slice(0, start) + this.line.slice(this.cursor);
          this.cursor = start;
        }
        return true;
      }
      default:
        return false;
    }
  }

  private recallPreviousHistory(): boolean {
    if (this.history.length === 0) return false;
    if (this.historyIndex === null) {
      this.historyDraft = this.line;
      this.historyIndex = this.history.length - 1;
    } else {
      this.historyIndex = Math.max(0, this.historyIndex - 1);
    }
    this.line = this.history[this.historyIndex];
    this.cursor = this.line.length;
    return true;
  }

  private recallNextHistory(): boolean {
    if (this.historyIndex === null) return false;
    if (this.historyIndex < this.history.length - 1) {
      this.historyIndex++;
      this.line = this.history[this.historyIndex];
    } else {
      this.historyIndex = null;
      this.line = this.historyDraft;
    }
    this.cursor = this.line.length;
    return true;
  }
}
