use crate::lexer::LexTokenV1;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedLineV1 {
    pub stages: Vec<ParsedCommandV1>,
    pub redirect: Option<ParsedRedirectV1>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedCommandV1 {
    pub name: String,
    pub arguments: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParsedRedirectModeV1 {
    Overwrite,
    Append,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ParsedRedirectV1 {
    pub mode: ParsedRedirectModeV1,
    pub path: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ParseErrorV1 {
    EmptyPipelineStage,
    MissingRedirectTarget,
    MultipleRedirects,
    RedirectNotFinal,
}

pub fn parse_p0_tokens(tokens: &[LexTokenV1]) -> Result<ParsedLineV1, ParseErrorV1> {
    let mut stages = Vec::new();
    let mut current_words = Vec::new();
    let mut redirect_mode = None;
    let mut redirect_path = None;

    for token in tokens {
        match token {
            LexTokenV1::Word(value) => {
                if redirect_mode.is_some() {
                    if redirect_path.is_some() {
                        return Err(ParseErrorV1::RedirectNotFinal);
                    }
                    redirect_path = Some(value.clone());
                } else {
                    current_words.push(value.clone());
                }
            }
            LexTokenV1::Pipe => {
                if redirect_mode.is_some() {
                    return Err(ParseErrorV1::RedirectNotFinal);
                }
                push_stage(&mut stages, &mut current_words)?;
            }
            LexTokenV1::RedirectOverwrite | LexTokenV1::RedirectAppend => {
                if redirect_mode.is_some() {
                    return Err(ParseErrorV1::MultipleRedirects);
                }
                if current_words.is_empty() {
                    return Err(ParseErrorV1::EmptyPipelineStage);
                }
                redirect_mode = Some(match token {
                    LexTokenV1::RedirectOverwrite => ParsedRedirectModeV1::Overwrite,
                    LexTokenV1::RedirectAppend => ParsedRedirectModeV1::Append,
                    LexTokenV1::Word(_) | LexTokenV1::Pipe => unreachable!(),
                });
            }
        }
    }

    if redirect_mode.is_some() && redirect_path.is_none() {
        return Err(ParseErrorV1::MissingRedirectTarget);
    }
    push_stage(&mut stages, &mut current_words)?;
    Ok(ParsedLineV1 {
        stages,
        redirect: redirect_mode.map(|mode| ParsedRedirectV1 {
            mode,
            path: redirect_path.expect("redirect mode requires a path"),
        }),
    })
}

fn push_stage(
    stages: &mut Vec<ParsedCommandV1>,
    words: &mut Vec<String>,
) -> Result<(), ParseErrorV1> {
    if words.is_empty() {
        return Err(ParseErrorV1::EmptyPipelineStage);
    }
    let mut values = std::mem::take(words).into_iter();
    stages.push(ParsedCommandV1 {
        name: values.next().expect("non-empty stage"),
        arguments: values.collect(),
    });
    Ok(())
}
