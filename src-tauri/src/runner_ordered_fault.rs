use crate::ordered_pipeline::OrderedPipelineFaultV1;
use std::io;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum OrderedFaultResolutionV1 {
    Cancelled,
    Diagnostic(&'static str),
    RedirectOutput,
    Output { kind: io::ErrorKind },
    Overflow,
    Unsupported,
}

pub(crate) fn resolve_ordered_fault(
    fault: OrderedPipelineFaultV1,
    redirected: bool,
) -> OrderedFaultResolutionV1 {
    match fault {
        OrderedPipelineFaultV1::TailResource => {
            OrderedFaultResolutionV1::Diagnostic("wingman tail: buffer resource limit exceeded")
        }
        OrderedPipelineFaultV1::SortResource => OrderedFaultResolutionV1::Diagnostic(
            "wingman sort: materialization resource limit exceeded",
        ),
        OrderedPipelineFaultV1::InvalidNumeric => {
            OrderedFaultResolutionV1::Diagnostic("wingman sort: invalid numeric data")
        }
        OrderedPipelineFaultV1::Output { .. } if redirected => {
            OrderedFaultResolutionV1::RedirectOutput
        }
        OrderedPipelineFaultV1::Output { kind } => OrderedFaultResolutionV1::Output { kind },
        OrderedPipelineFaultV1::Overflow => OrderedFaultResolutionV1::Overflow,
        OrderedPipelineFaultV1::Unsupported => OrderedFaultResolutionV1::Unsupported,
        OrderedPipelineFaultV1::Cancelled => OrderedFaultResolutionV1::Cancelled,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classifies_every_non_output_fault() {
        assert_eq!(
            resolve_ordered_fault(OrderedPipelineFaultV1::TailResource, false),
            OrderedFaultResolutionV1::Diagnostic("wingman tail: buffer resource limit exceeded")
        );
        assert_eq!(
            resolve_ordered_fault(OrderedPipelineFaultV1::SortResource, false),
            OrderedFaultResolutionV1::Diagnostic(
                "wingman sort: materialization resource limit exceeded"
            )
        );
        assert_eq!(
            resolve_ordered_fault(OrderedPipelineFaultV1::InvalidNumeric, false),
            OrderedFaultResolutionV1::Diagnostic("wingman sort: invalid numeric data")
        );
        assert_eq!(
            resolve_ordered_fault(OrderedPipelineFaultV1::Overflow, false),
            OrderedFaultResolutionV1::Overflow
        );
        assert_eq!(
            resolve_ordered_fault(OrderedPipelineFaultV1::Unsupported, false),
            OrderedFaultResolutionV1::Unsupported
        );
        assert_eq!(
            resolve_ordered_fault(OrderedPipelineFaultV1::Cancelled, false),
            OrderedFaultResolutionV1::Cancelled
        );
    }

    #[test]
    fn preserves_output_kind_without_redirection() {
        assert_eq!(
            resolve_ordered_fault(
                OrderedPipelineFaultV1::Output {
                    kind: io::ErrorKind::BrokenPipe,
                },
                false,
            ),
            OrderedFaultResolutionV1::Output {
                kind: io::ErrorKind::BrokenPipe,
            }
        );
    }

    #[test]
    fn hides_output_kind_behind_redirect_failure_boundary() {
        assert_eq!(
            resolve_ordered_fault(
                OrderedPipelineFaultV1::Output {
                    kind: io::ErrorKind::PermissionDenied,
                },
                true,
            ),
            OrderedFaultResolutionV1::RedirectOutput
        );
    }
}
