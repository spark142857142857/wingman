import { TerminalInputParser, type TerminalInputAction } from "./terminal-input.ts";

function assertActions(label: string, actual: TerminalInputAction[], expected: TerminalInputAction[]) {
  if (JSON.stringify(actual) !== JSON.stringify(expected)) {
    throw new Error(`${label}\nexpected: ${JSON.stringify(expected)}\nactual:   ${JSON.stringify(actual)}`);
  }
}

const focusBeforeExit = new TerminalInputParser();
assertActions(
  "focus reports do not contaminate exit",
  focusBeforeExit.consume("\u001b[O\u001b[Iexit\r"),
  [{ type: "write", data: "exit" }, { type: "submit", line: "exit", reliable: true }],
);

const splitFocusReport = new TerminalInputParser();
assertActions("split focus prefix is buffered", splitFocusReport.consume("\u001b["), []);
assertActions(
  "split focus suffix is removed",
  splitFocusReport.consume("Iexit\r"),
  [{ type: "write", data: "exit" }, { type: "submit", line: "exit", reliable: true }],
);

const arrowBeforeCommand = new TerminalInputParser();
assertActions(
  "arrow escape sequence is forwarded but excluded from the command",
  arrowBeforeCommand.consume("\u001b[Aexit\r"),
  [{ type: "write", data: "\u001b[Aexit" }, { type: "submit", line: "exit", reliable: false }],
);

const editing = new TerminalInputParser();
assertActions(
  "backspace updates the submitted line",
  editing.consume("exot\u007f\u007fit\r"),
  [{ type: "write", data: "exot\u007f\u007fit" }, { type: "submit", line: "exit", reliable: true }],
);

const commandAfterFocus = new TerminalInputParser();
assertActions(
  "cmd mapping receives a clean command",
  commandAfterFocus.consume("\u001b[Ils\r"),
  [{ type: "write", data: "ls" }, { type: "submit", line: "ls", reliable: true }],
);

const cursorEditing = new TerminalInputParser();
assertActions(
  "cursor editing preserves the actual command order",
  cursorEditing.consume("ls\u001b[Dx\r"),
  [{ type: "write", data: "ls\u001b[Dx" }, { type: "submit", line: "lxs", reliable: true }],
);

const localHistory = new TerminalInputParser();
assertActions(
  "initial history command",
  localHistory.consume("ls\r"),
  [{ type: "write", data: "ls" }, { type: "submit", line: "ls", reliable: true }],
);
localHistory.commitSubmittedLine("dir");
assertActions(
  "history recalls the command actually sent to the shell",
  localHistory.consume("\u001b[A\r"),
  [{ type: "write", data: "\u001b[A" }, { type: "submit", line: "dir", reliable: true }],
);

const tabCompletion = new TerminalInputParser();
assertActions(
  "tab completion safely disables command rewriting",
  tabCompletion.consume("fi\t\r"),
  [{ type: "write", data: "fi\t" }, { type: "submit", line: "fi", reliable: false }],
);

const nativePasteSuspension = new TerminalInputParser();
assertActions(
  "text before a confirmed native paste is mirrored",
  nativePasteSuspension.consume("partial"),
  [{ type: "write", data: "partial" }],
);
nativePasteSuspension.suspendAfterNativePaste();
assertActions(
  "the next submission remains native after a confirmed line-breaking paste",
  nativePasteSuspension.consume("pwd\r"),
  [{ type: "write", data: "pwd" }, { type: "submit", line: "pwd", reliable: false }],
);

const lfSeparatedPaste = new TerminalInputParser();
assertActions(
  "LF-separated pasted commands are submitted one line at a time",
  lfSeparatedPaste.consume("familiar status\ncat sample.txt | wc -l\n"),
  [
    { type: "write", data: "familiar status" },
    { type: "submit", line: "familiar status", reliable: true },
    { type: "write", data: "cat sample.txt | wc -l" },
    { type: "submit", line: "cat sample.txt | wc -l", reliable: true },
  ],
);

const crlfSeparatedPaste = new TerminalInputParser();
assertActions(
  "CRLF-separated pasted commands submit exactly once per line",
  crlfSeparatedPaste.consume("familiar status\r\ncat sample.txt | wc -l\r\n"),
  [
    { type: "write", data: "familiar status" },
    { type: "submit", line: "familiar status", reliable: true },
    { type: "write", data: "cat sample.txt | wc -l" },
    { type: "submit", line: "cat sample.txt | wc -l", reliable: true },
  ],
);

const splitCrlfPaste = new TerminalInputParser();
assertActions(
  "CR at the end of one input chunk submits the line",
  splitCrlfPaste.consume("familiar status\r"),
  [
    { type: "write", data: "familiar status" },
    { type: "submit", line: "familiar status", reliable: true },
  ],
);
assertActions(
  "LF at the start of the next chunk does not submit twice",
  splitCrlfPaste.consume("\ncat sample.txt | wc -l\r"),
  [
    { type: "write", data: "cat sample.txt | wc -l" },
    { type: "submit", line: "cat sample.txt | wc -l", reliable: true },
  ],
);

const bracketedPaste = new TerminalInputParser();
assertActions(
  "bracketed multiline paste keeps reserved commands reliable",
  bracketedPaste.consume(
    "\u001b[200~familiar status\ncat sample.txt | grep TODO | head -n 1\u001b[201~\r",
  ),
  [
    { type: "write", data: "familiar status" },
    { type: "submit", line: "familiar status", reliable: true },
    { type: "write", data: "cat sample.txt | grep TODO | head -n 1" },
    {
      type: "submit",
      line: "cat sample.txt | grep TODO | head -n 1",
      reliable: true,
    },
  ],
);

console.log("Terminal input parser tests passed.");
