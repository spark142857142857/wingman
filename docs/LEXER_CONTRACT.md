# Lexer Contract

Status: current constrained P0 lexer contract, implemented by the release candidate.

## Tokens

```text
Token = Word(value) | Pipe | RedirectOverwrite | RedirectAppend
```

The lexer preserves word values and recognizes only pipeline and final-output
redirection operators. Command meaning is assigned later by the catalog.

## States

The scanner has three states: `Normal`, `SingleQuoted`, and `DoubleQuoted`.

| State | Whitespace | `|` | `>` / `>>` | `\` |
| --- | --- | --- | --- | --- |
| Normal | separates words | pipe | redirect | ordinary character |
| SingleQuoted | ordinary character | ordinary character | ordinary character | ordinary character |
| DoubleQuoted | ordinary character | ordinary character | ordinary character | ordinary character |

Outside quotes, ASCII space and tab separate words. Quotes are removed and
their contents are concatenated with adjacent unquoted or differently quoted
segments of the same word. Empty quoted words are preserved as empty arguments.

Backslash is always an ordinary character. Wingman never treats it as a shell
escape, so Windows paths such as `C:\logs\app.log` survive unchanged.

## Quotes

Single and double quotes both group an argument. Neither expands environment
variables, backslashes, or shell expressions. A quote must be closed by the
same quote kind. P0 does not offer an escape sequence for embedding the same
quote kind inside itself; use the other quote kind where possible.

```text
grep 'fatal error' app.log  -> one pattern argument
grep "A|B" app.log          -> `|` is pattern text
grep "unterminated          -> reject
```

## Operators and structure

`|`, `>`, and `>>` are operators only outside quotes. `>` and `>>` may have no
surrounding whitespace, but exactly one redirect must be final and have one
output-path word.

```text
grep TODO app.log>out.txt       -> valid
grep TODO app.log >> "out file" -> valid
grep TODO app.log > out | head  -> reject
```

In a claimed P0 line, unquoted `&&`, `||`, `;`, `&`, `<`, backticks, `$(`, and
stream-qualified redirects such as `2>` or `&>` are unsupported operators.
`$`, `%`, `^`, and `\` alone remain ordinary word characters; Wingman does no
shell-variable expansion. Parentheses are ordinary characters except as part
of `$(`.

## Line scope and errors

P0 accepts one submitted line only. Newline continuation and other control
characters are outside it; tab is whitespace outside quotes.

```text
LexError =
    UnclosedSingleQuote
  | UnclosedDoubleQuote
  | UnsupportedOperator
  | UnsupportedStreamRedirection
  | ControlCharacter

ParseError =
    EmptyPipelineStage
  | MissingRedirectTarget
  | MultipleRedirects
  | RedirectNotFinal
```

The classifier runs a minimal first-command scan before full P0 lexing. Thus a
non-P0 native line passes through even if its later syntax is not P0, while a
line claimed by a P0 first command receives a deterministic lexer or parser
diagnostic.
