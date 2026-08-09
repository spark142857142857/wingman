use wingman_lib::lexer::{lex_p0_line, LexErrorV1, LexTokenV1};

#[test]
fn words_quotes_windows_paths_and_adjacent_segments_are_preserved() {
    assert_eq!(
        lex_p0_line(r#"grep 'fatal error' "C:\logs\app.log" pre"mid"'post' "" "#).unwrap(),
        vec![
            word("grep"),
            word("fatal error"),
            word(r"C:\logs\app.log"),
            word("premidpost"),
            word(""),
        ]
    );
}

#[test]
fn pipe_and_redirect_are_operators_only_outside_quotes() {
    assert_eq!(
        lex_p0_line(r#"grep "A|B> C" app.log|head -n 1 >>"out file""#).unwrap(),
        vec![
            word("grep"),
            word("A|B> C"),
            word("app.log"),
            LexTokenV1::Pipe,
            word("head"),
            word("-n"),
            word("1"),
            LexTokenV1::RedirectAppend,
            word("out file"),
        ]
    );
}

#[test]
fn unsupported_shell_operators_and_control_characters_are_rejected() {
    for (line, expected) in [
        ("cat a && cat b", LexErrorV1::UnsupportedOperator),
        ("cat a || cat b", LexErrorV1::UnsupportedOperator),
        ("cat a; cat b", LexErrorV1::UnsupportedOperator),
        ("cat < in", LexErrorV1::UnsupportedOperator),
        ("echo `pwd`", LexErrorV1::UnsupportedOperator),
        ("echo $(pwd)", LexErrorV1::UnsupportedOperator),
        ("cat a 2>err", LexErrorV1::UnsupportedStreamRedirection),
        ("head -n 1>>out", LexErrorV1::UnsupportedStreamRedirection),
        ("cat a &>err", LexErrorV1::UnsupportedStreamRedirection),
        ("cat a\nhead", LexErrorV1::ControlCharacter),
    ] {
        assert_eq!(lex_p0_line(line), Err(expected), "line: {line}");
    }
}

#[test]
fn unclosed_quotes_are_distinct_errors() {
    assert_eq!(
        lex_p0_line("grep 'unterminated"),
        Err(LexErrorV1::UnclosedSingleQuote)
    );
    assert_eq!(
        lex_p0_line("grep \"unterminated"),
        Err(LexErrorV1::UnclosedDoubleQuote)
    );
}

fn word(value: &str) -> LexTokenV1 {
    LexTokenV1::Word(value.to_string())
}
