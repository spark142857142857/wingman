use crate::runner_ls::names_equal_ignore_case;
use std::io::{self, Write};
use std::path::Path;

const MAX_MUTATION_DIAGNOSTICS: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum MutationExecutionErrorV1 {
    Output { kind: io::ErrorKind },
}

#[derive(Default)]
pub(crate) struct MutationDiagnosticsV1 {
    emitted: usize,
}

impl MutationDiagnosticsV1 {
    pub(crate) fn operand(
        &mut self,
        writer: &mut impl Write,
        command: &str,
        display: &str,
        detail: &str,
    ) -> Result<(), MutationExecutionErrorV1> {
        if self.emitted < MAX_MUTATION_DIAGNOSTICS {
            write_diagnostic(writer, &format!("wingman {command}: {display}: {detail}"))?;
            self.emitted += 1;
        }
        Ok(())
    }
}

pub(crate) fn write_diagnostic(
    writer: &mut impl Write,
    diagnostic: &str,
) -> Result<(), MutationExecutionErrorV1> {
    writer
        .write_all(diagnostic.as_bytes())
        .and_then(|()| writer.write_all(b"\r\n"))
        .and_then(|()| writer.flush())
        .map_err(|error| MutationExecutionErrorV1::Output { kind: error.kind() })
}

pub(crate) fn path_is_same_or_descendant(candidate: &Path, ancestor: &Path) -> bool {
    let candidate = candidate.components().collect::<Vec<_>>();
    let ancestor = ancestor.components().collect::<Vec<_>>();
    candidate.len() >= ancestor.len()
        && candidate.iter().zip(ancestor).all(|(left, right)| {
            names_equal_ignore_case(
                &left.as_os_str().to_string_lossy(),
                &right.as_os_str().to_string_lossy(),
            )
        })
}
