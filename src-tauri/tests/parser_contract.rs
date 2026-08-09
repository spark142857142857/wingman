use wingman_lib::lexer::lex_p0_line;
use wingman_lib::parser::{
    parse_p0_tokens, ParseErrorV1, ParsedCommandV1, ParsedLineV1, ParsedRedirectModeV1,
    ParsedRedirectV1,
};

#[test]
fn pipeline_and_one_final_redirect_form_a_shell_independent_shape() {
    let tokens = lex_p0_line(r#"cat "app log" | head -n 5 >> "out file""#).unwrap();
    assert_eq!(
        parse_p0_tokens(&tokens).unwrap(),
        ParsedLineV1 {
            stages: vec![
                ParsedCommandV1 {
                    name: "cat".to_string(),
                    arguments: vec!["app log".to_string()],
                },
                ParsedCommandV1 {
                    name: "head".to_string(),
                    arguments: vec!["-n".to_string(), "5".to_string()],
                },
            ],
            redirect: Some(ParsedRedirectV1 {
                mode: ParsedRedirectModeV1::Append,
                path: "out file".to_string(),
            }),
        }
    );
}

#[test]
fn structural_errors_are_deterministic() {
    for (line, expected) in [
        ("| head", ParseErrorV1::EmptyPipelineStage),
        ("cat a |", ParseErrorV1::EmptyPipelineStage),
        ("cat a > ", ParseErrorV1::MissingRedirectTarget),
        ("cat a > out > other", ParseErrorV1::MultipleRedirects),
        ("cat a > out | head", ParseErrorV1::RedirectNotFinal),
        ("cat a > out extra", ParseErrorV1::RedirectNotFinal),
    ] {
        let tokens = lex_p0_line(line).unwrap();
        assert_eq!(parse_p0_tokens(&tokens), Err(expected), "line: {line}");
    }
}
