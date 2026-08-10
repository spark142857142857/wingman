use regex::{Regex, RegexBuilder};

pub const MAX_FIND_PATTERN_BYTES: usize = 16 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FindPatternErrorV1 {
    InvalidSyntax,
    ResourceLimit,
}

pub struct FindPatternV1 {
    regex: Regex,
}

impl FindPatternV1 {
    pub fn compile(pattern: &str, ignore_case: bool) -> Result<Self, FindPatternErrorV1> {
        if pattern.len() > MAX_FIND_PATTERN_BYTES || pattern.contains(['/', ':', '\0', '\r', '\n'])
        {
            return Err(FindPatternErrorV1::InvalidSyntax);
        }
        let expression = translate(pattern)?;
        let regex = RegexBuilder::new(&expression)
            .case_insensitive(ignore_case)
            .unicode(true)
            .size_limit(1024 * 1024)
            .dfa_size_limit(2 * 1024 * 1024)
            .build()
            .map_err(|error| {
                if error
                    .to_string()
                    .contains("compiled regex exceeds size limit")
                {
                    FindPatternErrorV1::ResourceLimit
                } else {
                    FindPatternErrorV1::InvalidSyntax
                }
            })?;
        Ok(Self { regex })
    }

    pub fn is_match(&self, basename: &str) -> bool {
        self.regex.is_match(basename)
    }
}

fn translate(pattern: &str) -> Result<String, FindPatternErrorV1> {
    let characters = pattern.char_indices().collect::<Vec<_>>();
    let mut expression = String::from(r"\A");
    let mut index = 0usize;
    while index < characters.len() {
        let (byte_index, character) = characters[index];
        match character {
            '*' => expression.push_str(".*"),
            '?' => expression.push('.'),
            '[' => {
                let end = validate_class(&characters, index + 1)?;
                let end_byte = characters
                    .get(end)
                    .map_or(pattern.len(), |(byte_index, _)| *byte_index);
                expression.push_str(&pattern[byte_index..end_byte]);
                index = end;
                continue;
            }
            '\\' => {
                let Some((_, escaped)) = characters.get(index + 1) else {
                    return Err(FindPatternErrorV1::InvalidSyntax);
                };
                if !matches!(escaped, '*' | '?' | '[' | ']' | '\\' | '-' | '^') {
                    return Err(FindPatternErrorV1::InvalidSyntax);
                }
                expression.push_str(&regex::escape(&escaped.to_string()));
                index += 2;
                continue;
            }
            _ => expression.push_str(&regex::escape(&character.to_string())),
        }
        index += 1;
    }
    expression.push_str(r"\z");
    Ok(expression)
}

fn validate_class(
    characters: &[(usize, char)],
    mut index: usize,
) -> Result<usize, FindPatternErrorV1> {
    if characters
        .get(index)
        .is_some_and(|(_, value)| *value == '^')
    {
        index += 1;
    }
    let mut members = Vec::new();
    let mut closed = false;
    while index < characters.len() {
        let character = characters[index].1;
        if character == ']' {
            closed = true;
            index += 1;
            break;
        }
        if character == '\\' {
            let Some((_, escaped)) = characters.get(index + 1) else {
                return Err(FindPatternErrorV1::InvalidSyntax);
            };
            if !matches!(escaped, ']' | '-' | '^' | '\\') {
                return Err(FindPatternErrorV1::InvalidSyntax);
            }
            members.push(ClassMemberV1::Scalar(*escaped));
            index += 2;
        } else if character == '-' {
            members.push(ClassMemberV1::Hyphen);
            index += 1;
        } else {
            members.push(ClassMemberV1::Scalar(character));
            index += 1;
        }
    }
    if !closed || members.is_empty() {
        return Err(FindPatternErrorV1::InvalidSyntax);
    }
    for (member_index, member) in members.iter().enumerate() {
        if *member != ClassMemberV1::Hyphen
            || member_index == 0
            || member_index + 1 == members.len()
        {
            continue;
        }
        let (ClassMemberV1::Scalar(start), ClassMemberV1::Scalar(end)) =
            (members[member_index - 1], members[member_index + 1])
        else {
            return Err(FindPatternErrorV1::InvalidSyntax);
        };
        if start > end {
            return Err(FindPatternErrorV1::InvalidSyntax);
        }
    }
    Ok(index)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassMemberV1 {
    Scalar(char),
    Hyphen,
}
