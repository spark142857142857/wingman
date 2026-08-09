use regex::{Regex, RegexBuilder};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum GrepPatternErrorV1 {
    InvalidSyntax,
    ResourceLimit,
}

pub struct GrepPatternV1 {
    regex: Regex,
}

impl GrepPatternV1 {
    pub fn compile(
        pattern: &str,
        fixed_strings: bool,
        ignore_case: bool,
    ) -> Result<Self, GrepPatternErrorV1> {
        let expression = if fixed_strings {
            regex::escape(pattern)
        } else {
            validate_portable_pattern(pattern)?;
            pattern.to_string()
        };
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
                    GrepPatternErrorV1::ResourceLimit
                } else {
                    GrepPatternErrorV1::InvalidSyntax
                }
            })?;
        Ok(Self { regex })
    }

    pub fn is_match(&self, record: &str) -> bool {
        self.regex.is_match(record)
    }
}

fn validate_portable_pattern(pattern: &str) -> Result<(), GrepPatternErrorV1> {
    let characters = pattern.char_indices().collect::<Vec<_>>();
    let mut index = 0usize;
    let mut can_repeat = false;
    while index < characters.len() {
        let (byte_index, character) = characters[index];
        match character {
            '^' => {
                if index != 0 {
                    return Err(GrepPatternErrorV1::InvalidSyntax);
                }
                can_repeat = false;
                index += 1;
            }
            '$' => {
                if index + 1 != characters.len() {
                    return Err(GrepPatternErrorV1::InvalidSyntax);
                }
                can_repeat = false;
                index += 1;
            }
            '*' => {
                if !can_repeat {
                    return Err(GrepPatternErrorV1::InvalidSyntax);
                }
                can_repeat = false;
                index += 1;
            }
            '[' => {
                index = validate_class(&characters, index + 1)?;
                can_repeat = true;
            }
            '\\' => {
                let Some((_, escaped)) = characters.get(index + 1) else {
                    return Err(GrepPatternErrorV1::InvalidSyntax);
                };
                if !matches!(escaped, '.' | '*' | '^' | '$' | '[' | ']' | '\\' | '-') {
                    return Err(GrepPatternErrorV1::InvalidSyntax);
                }
                can_repeat = true;
                index += 2;
            }
            '(' | ')' | '|' | '+' | '?' | '{' | '}' => {
                return Err(GrepPatternErrorV1::InvalidSyntax);
            }
            _ => {
                let _ = byte_index;
                can_repeat = true;
                index += 1;
            }
        }
    }
    Ok(())
}

fn validate_class(
    characters: &[(usize, char)],
    mut index: usize,
) -> Result<usize, GrepPatternErrorV1> {
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
                return Err(GrepPatternErrorV1::InvalidSyntax);
            };
            if !matches!(escaped, ']' | '-' | '^' | '\\') {
                return Err(GrepPatternErrorV1::InvalidSyntax);
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
        return Err(GrepPatternErrorV1::InvalidSyntax);
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
            return Err(GrepPatternErrorV1::InvalidSyntax);
        };
        if start > end {
            return Err(GrepPatternErrorV1::InvalidSyntax);
        }
    }
    Ok(index)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum ClassMemberV1 {
    Scalar(char),
    Hyphen,
}
