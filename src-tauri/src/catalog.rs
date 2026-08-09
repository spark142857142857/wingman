use crate::grep_pattern::GrepPatternV1;
use crate::interpreter::{
    validate_execution_plan, ExecutionPlanV1, RedirectModeV1, RunnerRequestValidationErrorV1,
    StagePlanV1, ValidatedRedirectPlanV1,
};
use crate::parser::{ParsedCommandV1, ParsedLineV1, ParsedRedirectModeV1};
use crate::windows_path::{validate_path_value, PathValidationErrorV1};

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CatalogErrorV1 {
    UnsupportedCommand,
    UnsupportedOption,
    MissingOperand,
    InvalidCount,
    InvalidSourceShape,
    InvalidPattern,
    ResourceLimit,
    Path(PathValidationErrorV1),
}

pub fn build_readonly_plan(parsed: &ParsedLineV1) -> Result<ExecutionPlanV1, CatalogErrorV1> {
    let mut stages = Vec::with_capacity(parsed.stages.len());
    for (index, command) in parsed.stages.iter().enumerate() {
        if command.name.eq_ignore_ascii_case("cat") {
            if index != 0 {
                return Err(CatalogErrorV1::InvalidSourceShape);
            }
            stages.push(build_cat(command)?);
        } else if command.name.eq_ignore_ascii_case("head") {
            stages.push(build_head(command, index > 0)?);
        } else if command.name.eq_ignore_ascii_case("tail") {
            stages.push(build_tail(command, index > 0)?);
        } else if command.name.eq_ignore_ascii_case("wc") {
            stages.push(build_wc(command, index > 0)?);
        } else if command.name.eq_ignore_ascii_case("grep") {
            stages.push(build_grep(command, index > 0)?);
        } else if command.name.eq_ignore_ascii_case("uniq") {
            stages.push(build_uniq(command, index > 0)?);
        } else if command.name.eq_ignore_ascii_case("sort") {
            stages.push(build_sort(command, index > 0)?);
        } else {
            return Err(CatalogErrorV1::UnsupportedCommand);
        }
    }
    let redirect = parsed
        .redirect
        .as_ref()
        .map(|redirect| {
            Ok(ValidatedRedirectPlanV1 {
                mode: match redirect.mode {
                    ParsedRedirectModeV1::Overwrite => RedirectModeV1::Overwrite,
                    ParsedRedirectModeV1::Append => RedirectModeV1::Append,
                },
                path: validate_path_value(&redirect.path).map_err(CatalogErrorV1::Path)?,
            })
        })
        .transpose()?;
    let plan = ExecutionPlanV1 { stages, redirect };
    validate_execution_plan(&plan).map_err(|error| match error {
        RunnerRequestValidationErrorV1::InvalidRange => CatalogErrorV1::InvalidCount,
        RunnerRequestValidationErrorV1::InvalidStageCount
        | RunnerRequestValidationErrorV1::InvalidPathCount => CatalogErrorV1::ResourceLimit,
        _ => CatalogErrorV1::InvalidSourceShape,
    })?;
    Ok(plan)
}

fn build_sort(
    command: &ParsedCommandV1,
    has_pipeline_input: bool,
) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut reverse = false;
    let mut numeric = false;
    let mut unique = false;
    let mut parse_options = true;
    let mut paths = Vec::new();
    for argument in &command.arguments {
        if parse_options && argument == "--" {
            parse_options = false;
        } else if parse_options {
            if let Some(long) = argument.strip_prefix("--") {
                match long {
                    "reverse" => reverse = true,
                    "numeric-sort" => numeric = true,
                    "unique" => unique = true,
                    _ => return Err(CatalogErrorV1::UnsupportedOption),
                }
            } else if let Some(shorts) = argument.strip_prefix('-') {
                if shorts.is_empty() {
                    paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
                    continue;
                }
                for short in shorts.chars() {
                    match short {
                        'r' => reverse = true,
                        'n' => numeric = true,
                        'u' => unique = true,
                        _ => return Err(CatalogErrorV1::UnsupportedOption),
                    }
                }
            } else {
                paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
            }
        } else {
            paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
        }
    }
    let path = if has_pipeline_input {
        if !paths.is_empty() {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        None
    } else {
        if paths.len() != 1 {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        paths.pop()
    };
    Ok(StagePlanV1::SortLines {
        path,
        reverse,
        numeric,
        unique,
    })
}

fn build_uniq(
    command: &ParsedCommandV1,
    has_pipeline_input: bool,
) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut count = false;
    let mut repeated_only = false;
    let mut unique_only = false;
    let mut parse_options = true;
    let mut paths = Vec::new();
    for argument in &command.arguments {
        if parse_options && argument == "--" {
            parse_options = false;
        } else if parse_options {
            if let Some(long) = argument.strip_prefix("--") {
                match long {
                    "count" => count = true,
                    "repeated" => repeated_only = true,
                    "unique" => unique_only = true,
                    _ => return Err(CatalogErrorV1::UnsupportedOption),
                }
            } else if let Some(shorts) = argument.strip_prefix('-') {
                if shorts.is_empty() {
                    paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
                    continue;
                }
                for short in shorts.chars() {
                    match short {
                        'c' => count = true,
                        'd' => repeated_only = true,
                        'u' => unique_only = true,
                        _ => return Err(CatalogErrorV1::UnsupportedOption),
                    }
                }
            } else {
                paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
            }
        } else {
            paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
        }
    }
    if repeated_only && unique_only {
        return Err(CatalogErrorV1::UnsupportedOption);
    }
    let path = if has_pipeline_input {
        if !paths.is_empty() {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        None
    } else {
        if paths.len() != 1 {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        paths.pop()
    };
    Ok(StagePlanV1::UniqueLines {
        path,
        count,
        repeated_only,
        unique_only,
    })
}

fn build_grep(
    command: &ParsedCommandV1,
    has_pipeline_input: bool,
) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut ignore_case = false;
    let mut line_numbers = false;
    let mut invert_match = false;
    let mut fixed_strings = false;
    let mut recursive = false;
    let mut parse_options = true;
    let mut index = 0usize;
    while parse_options && index < command.arguments.len() {
        let argument = &command.arguments[index];
        if argument == "--" {
            parse_options = false;
            index += 1;
        } else if let Some(long) = argument.strip_prefix("--") {
            match long {
                "ignore-case" => ignore_case = true,
                "line-number" => line_numbers = true,
                "invert-match" => invert_match = true,
                "fixed-strings" => fixed_strings = true,
                "recursive" => recursive = true,
                _ => return Err(CatalogErrorV1::UnsupportedOption),
            }
            index += 1;
        } else if let Some(shorts) = argument.strip_prefix('-') {
            if shorts.is_empty() {
                break;
            }
            for short in shorts.chars() {
                match short {
                    'i' => ignore_case = true,
                    'n' => line_numbers = true,
                    'v' => invert_match = true,
                    'F' => fixed_strings = true,
                    'r' => recursive = true,
                    _ => return Err(CatalogErrorV1::UnsupportedOption),
                }
            }
            index += 1;
        } else {
            break;
        }
    }
    let pattern = command
        .arguments
        .get(index)
        .ok_or(CatalogErrorV1::MissingOperand)?
        .clone();
    index += 1;
    GrepPatternV1::compile(&pattern, fixed_strings, ignore_case)
        .map_err(|_| CatalogErrorV1::InvalidPattern)?;

    let paths = command.arguments[index..]
        .iter()
        .map(|path| validate_path_value(path).map_err(CatalogErrorV1::Path))
        .collect::<Result<Vec<_>, _>>()?;
    if has_pipeline_input {
        if !paths.is_empty() || recursive {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
    } else if paths.is_empty() {
        return Err(CatalogErrorV1::InvalidSourceShape);
    }
    Ok(StagePlanV1::SearchText {
        pattern,
        paths,
        ignore_case,
        line_numbers,
        invert_match,
        fixed_strings,
        recursive,
    })
}

fn build_cat(command: &ParsedCommandV1) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut number_lines = false;
    let mut parse_options = true;
    let mut paths = Vec::new();
    for argument in &command.arguments {
        if parse_options && argument == "--" {
            parse_options = false;
        } else if parse_options && matches!(argument.as_str(), "-n" | "--number") {
            number_lines = true;
        } else if parse_options && argument.starts_with('-') {
            return Err(CatalogErrorV1::UnsupportedOption);
        } else {
            paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
        }
    }
    if paths.is_empty() {
        return Err(CatalogErrorV1::MissingOperand);
    }
    Ok(StagePlanV1::ReadTextFiles {
        paths,
        number_lines,
    })
}

fn build_head(
    command: &ParsedCommandV1,
    has_pipeline_input: bool,
) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut count = 10usize;
    let mut index = 0;
    let mut parse_options = true;
    let mut paths = Vec::new();
    while index < command.arguments.len() {
        let argument = &command.arguments[index];
        if parse_options && argument == "--" {
            parse_options = false;
            index += 1;
        } else if parse_options && argument == "-n" {
            let value = command
                .arguments
                .get(index + 1)
                .ok_or(CatalogErrorV1::InvalidCount)?;
            count = value
                .parse::<usize>()
                .map_err(|_| CatalogErrorV1::InvalidCount)?;
            index += 2;
        } else if parse_options && argument.starts_with('-') {
            return Err(CatalogErrorV1::UnsupportedOption);
        } else {
            paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
            index += 1;
        }
    }
    let path = if has_pipeline_input {
        if !paths.is_empty() {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        None
    } else {
        if paths.len() != 1 {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        paths.pop()
    };
    Ok(StagePlanV1::HeadLines { count, path })
}

fn build_wc(
    command: &ParsedCommandV1,
    has_pipeline_input: bool,
) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut lines = false;
    let mut parse_options = true;
    let mut paths = Vec::new();
    for argument in &command.arguments {
        if parse_options && argument == "--" {
            parse_options = false;
        } else if parse_options && matches!(argument.as_str(), "-l" | "--lines") {
            lines = true;
        } else if parse_options && argument.starts_with('-') {
            return Err(CatalogErrorV1::UnsupportedOption);
        } else {
            paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
        }
    }
    if !lines {
        return Err(CatalogErrorV1::UnsupportedOption);
    }
    let path = if has_pipeline_input {
        if !paths.is_empty() {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        None
    } else {
        if paths.len() != 1 {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        paths.pop()
    };
    Ok(StagePlanV1::CountLines { path })
}

fn build_tail(
    command: &ParsedCommandV1,
    has_pipeline_input: bool,
) -> Result<StagePlanV1, CatalogErrorV1> {
    let mut count = 10usize;
    let mut index = 0;
    let mut parse_options = true;
    let mut paths = Vec::new();
    while index < command.arguments.len() {
        let argument = &command.arguments[index];
        if parse_options && argument == "--" {
            parse_options = false;
            index += 1;
        } else if parse_options && argument == "-n" {
            let value = command
                .arguments
                .get(index + 1)
                .ok_or(CatalogErrorV1::InvalidCount)?;
            count = value
                .parse::<usize>()
                .map_err(|_| CatalogErrorV1::InvalidCount)?;
            index += 2;
        } else if parse_options && argument.starts_with('-') {
            return Err(CatalogErrorV1::UnsupportedOption);
        } else {
            paths.push(validate_path_value(argument).map_err(CatalogErrorV1::Path)?);
            index += 1;
        }
    }
    let path = if has_pipeline_input {
        if !paths.is_empty() {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        None
    } else {
        if paths.len() != 1 {
            return Err(CatalogErrorV1::InvalidSourceShape);
        }
        paths.pop()
    };
    Ok(StagePlanV1::TailLines { count, path })
}
