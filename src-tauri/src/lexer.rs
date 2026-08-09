#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LexTokenV1 {
    Word(String),
    Pipe,
    RedirectOverwrite,
    RedirectAppend,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LexErrorV1 {
    UnclosedSingleQuote,
    UnclosedDoubleQuote,
    UnsupportedOperator,
    UnsupportedStreamRedirection,
    ControlCharacter,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum LexerState {
    Normal,
    SingleQuoted,
    DoubleQuoted,
}

pub fn lex_p0_line(line: &str) -> Result<Vec<LexTokenV1>, LexErrorV1> {
    let mut tokens = Vec::new();
    let mut word = String::new();
    let mut word_started = false;
    let mut word_contains_quotes = false;
    let mut state = LexerState::Normal;
    let mut characters = line.chars().peekable();

    while let Some(character) = characters.next() {
        if character < ' ' && character != '\t' {
            return Err(LexErrorV1::ControlCharacter);
        }
        match state {
            LexerState::SingleQuoted => {
                if character == '\'' {
                    state = LexerState::Normal;
                } else {
                    word.push(character);
                }
            }
            LexerState::DoubleQuoted => {
                if character == '"' {
                    state = LexerState::Normal;
                } else {
                    word.push(character);
                }
            }
            LexerState::Normal => match character {
                ' ' | '\t' => {
                    flush_word(&mut tokens, &mut word, &mut word_started);
                    word_contains_quotes = false;
                }
                '\'' => {
                    state = LexerState::SingleQuoted;
                    word_started = true;
                    word_contains_quotes = true;
                }
                '"' => {
                    state = LexerState::DoubleQuoted;
                    word_started = true;
                    word_contains_quotes = true;
                }
                '|' => {
                    if characters.peek() == Some(&'|') {
                        return Err(LexErrorV1::UnsupportedOperator);
                    }
                    flush_word(&mut tokens, &mut word, &mut word_started);
                    word_contains_quotes = false;
                    tokens.push(LexTokenV1::Pipe);
                }
                '>' => {
                    if word_started
                        && !word_contains_quotes
                        && word.chars().all(|value| value.is_ascii_digit())
                    {
                        return Err(LexErrorV1::UnsupportedStreamRedirection);
                    }
                    flush_word(&mut tokens, &mut word, &mut word_started);
                    word_contains_quotes = false;
                    if characters.peek() == Some(&'>') {
                        characters.next();
                        tokens.push(LexTokenV1::RedirectAppend);
                    } else {
                        tokens.push(LexTokenV1::RedirectOverwrite);
                    }
                }
                '&' => {
                    if characters.peek() == Some(&'>') {
                        return Err(LexErrorV1::UnsupportedStreamRedirection);
                    }
                    return Err(LexErrorV1::UnsupportedOperator);
                }
                '<' | ';' | '`' => return Err(LexErrorV1::UnsupportedOperator),
                '$' if characters.peek() == Some(&'(') => {
                    return Err(LexErrorV1::UnsupportedOperator);
                }
                _ => {
                    word.push(character);
                    word_started = true;
                }
            },
        }
    }

    match state {
        LexerState::SingleQuoted => return Err(LexErrorV1::UnclosedSingleQuote),
        LexerState::DoubleQuoted => return Err(LexErrorV1::UnclosedDoubleQuote),
        LexerState::Normal => {}
    }
    flush_word(&mut tokens, &mut word, &mut word_started);
    Ok(tokens)
}

fn flush_word(tokens: &mut Vec<LexTokenV1>, word: &mut String, word_started: &mut bool) {
    if *word_started {
        tokens.push(LexTokenV1::Word(std::mem::take(word)));
        *word_started = false;
    }
}
