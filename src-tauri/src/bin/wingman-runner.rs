use std::io::{self, Write};
use wingman_lib::interpreter::decode_prepared_request;
use wingman_lib::runner::{execute_prepared_to_with_cancellation, RunnerDispatchErrorV1};
use wingman_lib::runner_cancel::ConsoleCancellationGuardV1;
use wingman_lib::transport::fetch_prepared_request_channel;

fn main() {
    let Some(pipe_name) = std::env::var_os("WINGMAN_BROKER_PIPE") else {
        let _ = io::stderr().write_all(b"wingman-runner: broker endpoint is unavailable\r\n");
        std::process::exit(2);
    };
    let mut args = std::env::args_os().skip(1);
    let Some(request_id) = args.next().and_then(|value| value.into_string().ok()) else {
        fail_transport();
    };
    if args.next().is_some() {
        fail_transport();
    }

    let channel = fetch_prepared_request_channel(&pipe_name, &request_id)
        .unwrap_or_else(|_| fail_transport());
    let (wire, cancellation_receiver) = channel.into_parts();
    let request = decode_prepared_request(&wire).unwrap_or_else(|_| fail_request());
    let (_cancellation_guard, cancellation) =
        ConsoleCancellationGuardV1::install().unwrap_or_else(|_| fail_request());
    let channel_cancellation = cancellation.clone();
    let _channel_watcher = std::thread::spawn(move || {
        if matches!(cancellation_receiver.wait(), Ok(true)) {
            channel_cancellation.cancel();
        }
    });
    let mut stdout = io::stdout().lock();
    let mut stderr = io::stderr().lock();
    let exit_code = match execute_prepared_to_with_cancellation(
        request,
        &mut stdout,
        &mut stderr,
        &cancellation,
    ) {
        Ok(exit_code) => exit_code,
        Err(RunnerDispatchErrorV1::OutputFailure { .. }) => 1,
        Err(_) => fail_request(),
    };
    std::process::exit(exit_code.into());
}

fn fail_transport() -> ! {
    let _ = io::stderr().write_all(b"wingman-runner: transport is unavailable\r\n");
    std::process::exit(2);
}

fn fail_request() -> ! {
    let _ = io::stderr().write_all(b"wingman-runner: prepared request was rejected\r\n");
    std::process::exit(2);
}
