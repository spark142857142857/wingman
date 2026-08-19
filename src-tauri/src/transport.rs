#[cfg(windows)]
mod windows {
    use crate::interpreter::{ActiveShell, PreparedRequestV1, MAX_PREPARED_REQUEST_BYTES};
    use std::collections::HashMap;
    use std::ffi::OsStr;
    use std::fs::File;
    use std::io::{self, Read, Write};
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use std::ptr::{null, null_mut};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::mpsc::{self, Receiver, SyncSender, TryRecvError};
    use std::sync::{Arc, Mutex};
    use std::thread::{self, JoinHandle};
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{
        GetLastError, LocalFree, ERROR_FILE_NOT_FOUND, ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED,
        GENERIC_READ, GENERIC_WRITE, INVALID_HANDLE_VALUE,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
    };
    use windows_sys::Win32::Security::{PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES};
    use windows_sys::Win32::Storage::FileSystem::{
        CreateFileW, FILE_FLAGS_AND_ATTRIBUTES, OPEN_EXISTING, PIPE_ACCESS_DUPLEX,
    };
    use windows_sys::Win32::System::Pipes::{
        ConnectNamedPipe, CreateNamedPipeW, PeekNamedPipe, WaitNamedPipeW, PIPE_READMODE_BYTE,
        PIPE_REJECT_REMOTE_CLIENTS, PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
    };

    const REQUEST_ID_BYTES: usize = 32;
    const PIPE_BUFFER_BYTES: u32 = 64 * 1024;
    const PIPE_WAIT_MILLIS: u32 = 5_000;
    const PIPE_DACL: &str = "D:P(A;;GA;;;SY)(A;;GA;;;OW)";
    const DEFAULT_PREPARED_REQUEST_TTL: Duration = Duration::from_secs(30);
    const MAX_PENDING_REQUESTS: usize = 128;
    const REQUEST_FRAME_BYTES: u32 = (REQUEST_ID_BYTES + 1) as u32;
    const CONNECTION_READ_TIMEOUT: Duration = Duration::from_secs(1);
    const CONNECTION_POLL_INTERVAL: Duration = Duration::from_millis(10);
    const READINESS_AUTH_TIMEOUT: Duration = Duration::from_secs(5);
    const MAX_READINESS_FRAME_BYTES: usize = 256;
    const MAX_PENDING_READINESS_FRAMES: usize = 8;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum EditorLocationKindV1 {
        FileSystem,
        NonFileSystem,
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    pub enum EditorAdapterCapabilityV1 {
        PsReadLineReplaceV1,
    }

    pub const MAX_POWERSHELL_NESTED_DEPTH: u32 = 16;

    #[derive(Clone, Debug, Eq, PartialEq)]
    pub struct EditorReadinessFrameV1 {
        pub nonce: String,
        pub sequence: u64,
        pub shell: ActiveShell,
        pub shell_depth: u32,
        pub location_kind: EditorLocationKindV1,
        pub adapter_capability: EditorAdapterCapabilityV1,
    }

    pub fn parse_editor_readiness_frame(line: &str) -> io::Result<EditorReadinessFrameV1> {
        if line.is_empty()
            || line.len() > MAX_READINESS_FRAME_BYTES
            || !line.is_ascii()
            || line.contains(['\r', '\n'])
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid editor readiness frame",
            ));
        }
        let fields: Vec<&str> = line.split(';').collect();
        if fields.len() != 7
            || fields[0] != "1"
            || !is_valid_nonce(fields[1])
            || fields[2].is_empty()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid editor readiness frame",
            ));
        }
        let sequence = fields[2].parse::<u64>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid editor readiness sequence",
            )
        })?;
        if sequence == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid editor readiness sequence",
            ));
        }
        let shell = match fields[3] {
            "powershell" => ActiveShell::WindowsPowerShell,
            "cmd" => ActiveShell::Cmd,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid editor readiness shell",
                ))
            }
        };
        let shell_depth = fields[4].parse::<u32>().map_err(|_| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                "invalid editor readiness shell depth",
            )
        })?;
        if shell_depth > MAX_POWERSHELL_NESTED_DEPTH {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "editor readiness shell depth exceeds the supported limit",
            ));
        }
        let location_kind = match fields[5] {
            "filesystem" => EditorLocationKindV1::FileSystem,
            "non-filesystem" => EditorLocationKindV1::NonFileSystem,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid editor readiness location",
                ))
            }
        };
        let adapter_capability = match fields[6] {
            "psreadline-replace-v1" => EditorAdapterCapabilityV1::PsReadLineReplaceV1,
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "invalid editor readiness capability",
                ))
            }
        };
        Ok(EditorReadinessFrameV1 {
            nonce: fields[1].to_string(),
            sequence,
            shell,
            shell_depth,
            location_kind,
            adapter_capability,
        })
    }

    pub struct OneShotBrokerV1 {
        pipe: File,
        request_id: String,
        request: PreparedRequestV1,
        expires_at: Instant,
    }

    impl AsRawHandle for OneShotBrokerV1 {
        fn as_raw_handle(&self) -> std::os::windows::io::RawHandle {
            self.pipe.as_raw_handle()
        }
    }

    impl OneShotBrokerV1 {
        pub fn bind(
            pipe_name: &str,
            request_id: String,
            request: PreparedRequestV1,
        ) -> io::Result<Self> {
            Self::bind_with_ttl(pipe_name, request_id, request, DEFAULT_PREPARED_REQUEST_TTL)
        }

        pub fn bind_with_ttl(
            pipe_name: &str,
            request_id: String,
            request: PreparedRequestV1,
            ttl: Duration,
        ) -> io::Result<Self> {
            validate_request_id(&request_id)?;
            let expires_at = Instant::now().checked_add(ttl).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "prepared request TTL is too large",
                )
            })?;
            let pipe = create_server_pipe(pipe_name)?;
            Ok(Self {
                pipe,
                request_id,
                request,
                expires_at,
            })
        }

        pub fn serve(mut self) -> io::Result<()> {
            let connected =
                unsafe { ConnectNamedPipe(self.pipe.as_raw_handle().cast(), null_mut()) };
            if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
                return Err(io::Error::last_os_error());
            }

            let received_id = read_request_id(&mut self.pipe)?;
            if received_id != self.request_id {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "unknown prepared request",
                ));
            }
            if Instant::now() >= self.expires_at {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "prepared request has expired",
                ));
            }

            let wire = serde_json::to_vec(&self.request).map_err(io::Error::other)?;
            if wire.len() > MAX_PREPARED_REQUEST_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "prepared request exceeds transport limit",
                ));
            }
            self.pipe.write_all(&(wire.len() as u32).to_le_bytes())?;
            self.pipe.write_all(&wire)?;
            self.pipe.flush()
        }
    }

    struct StoredPreparedRequest {
        request: PreparedRequestV1,
        expires_at: Instant,
    }

    struct SessionBrokerShared {
        pipe_name: String,
        stopped: AtomicBool,
        worker_alive: AtomicBool,
        request_ttl: Duration,
        requests: Mutex<HashMap<String, StoredPreparedRequest>>,
    }

    pub struct SessionBrokerV1 {
        shared: Arc<SessionBrokerShared>,
        worker: Option<JoinHandle<io::Result<()>>>,
    }

    impl SessionBrokerV1 {
        pub fn start(pipe_name: &str) -> io::Result<Self> {
            Self::start_with_ttl(pipe_name, DEFAULT_PREPARED_REQUEST_TTL)
        }

        pub fn start_with_ttl(pipe_name: &str, request_ttl: Duration) -> io::Result<Self> {
            if request_ttl.is_zero() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "session broker request TTL must be positive",
                ));
            }
            let first_pipe = create_server_pipe(pipe_name)?;
            let shared = Arc::new(SessionBrokerShared {
                pipe_name: pipe_name.to_string(),
                stopped: AtomicBool::new(false),
                worker_alive: AtomicBool::new(true),
                request_ttl,
                requests: Mutex::new(HashMap::new()),
            });
            let worker_shared = shared.clone();
            let worker = thread::spawn(move || {
                let result = serve_session_broker(worker_shared.clone(), first_pipe);
                worker_shared.worker_alive.store(false, Ordering::Release);
                result
            });
            Ok(Self {
                shared,
                worker: Some(worker),
            })
        }

        pub fn register(&self, request_id: String, request: PreparedRequestV1) -> io::Result<()> {
            validate_request_id(&request_id)?;
            if self.shared.stopped.load(Ordering::Acquire)
                || !self.shared.worker_alive.load(Ordering::Acquire)
            {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "session broker is not serving",
                ));
            }
            let wire = serde_json::to_vec(&request).map_err(io::Error::other)?;
            if wire.len() > MAX_PREPARED_REQUEST_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "prepared request exceeds transport limit",
                ));
            }
            let expires_at = Instant::now()
                .checked_add(self.shared.request_ttl)
                .ok_or_else(|| io::Error::other("prepared request TTL overflow"))?;
            let mut requests = self
                .shared
                .requests
                .lock()
                .map_err(|_| io::Error::other("session broker registry is poisoned"))?;
            let now = Instant::now();
            requests.retain(|_, stored| stored.expires_at > now);
            if requests.len() >= MAX_PENDING_REQUESTS {
                return Err(io::Error::new(
                    io::ErrorKind::OutOfMemory,
                    "session broker request registry is full",
                ));
            }
            if requests.contains_key(&request_id) {
                return Err(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "prepared request ID is already registered",
                ));
            }
            requests.insert(
                request_id,
                StoredPreparedRequest {
                    request,
                    expires_at,
                },
            );
            Ok(())
        }

        pub fn unregister(&self, request_id: &str) -> io::Result<bool> {
            validate_request_id(request_id)?;
            Ok(self
                .shared
                .requests
                .lock()
                .map_err(|_| io::Error::other("session broker registry is poisoned"))?
                .remove(request_id)
                .is_some())
        }

        pub fn stop(mut self) -> io::Result<()> {
            self.stop_and_join()
        }

        fn stop_and_join(&mut self) -> io::Result<()> {
            self.shared.stopped.store(true, Ordering::Release);
            let _ = wake_session_broker(&self.shared.pipe_name);
            if let Some(worker) = self.worker.take() {
                worker
                    .join()
                    .map_err(|_| io::Error::other("session broker thread panicked"))??;
            }
            self.shared
                .requests
                .lock()
                .map_err(|_| io::Error::other("session broker registry is poisoned"))?
                .clear();
            Ok(())
        }
    }

    impl Drop for SessionBrokerV1 {
        fn drop(&mut self) {
            let _ = self.stop_and_join();
        }
    }

    struct EditorReadinessShared {
        pipe_name: String,
        expected_nonce: String,
        stopped: AtomicBool,
        worker_alive: AtomicBool,
        poisoned: AtomicBool,
    }

    pub struct EditorReadinessBrokerV1 {
        shared: Arc<EditorReadinessShared>,
        receiver: Mutex<Receiver<EditorReadinessFrameV1>>,
        worker: Option<JoinHandle<io::Result<()>>>,
    }

    impl EditorReadinessBrokerV1 {
        pub fn start(pipe_name: &str, expected_nonce: String) -> io::Result<Self> {
            if !is_valid_nonce(&expected_nonce) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "invalid editor readiness nonce",
                ));
            }
            let first_pipe = create_server_pipe(pipe_name)?;
            let (sender, receiver) = mpsc::sync_channel(MAX_PENDING_READINESS_FRAMES);
            let shared = Arc::new(EditorReadinessShared {
                pipe_name: pipe_name.to_string(),
                expected_nonce,
                stopped: AtomicBool::new(false),
                worker_alive: AtomicBool::new(true),
                poisoned: AtomicBool::new(false),
            });
            let worker_shared = shared.clone();
            let worker = thread::spawn(move || {
                let result = serve_editor_readiness(worker_shared.clone(), first_pipe, sender);
                worker_shared.worker_alive.store(false, Ordering::Release);
                result
            });
            Ok(Self {
                shared,
                receiver: Mutex::new(receiver),
                worker: Some(worker),
            })
        }

        pub fn drain(&self) -> io::Result<Vec<EditorReadinessFrameV1>> {
            if self.shared.poisoned.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "editor readiness session is poisoned",
                ));
            }
            let receiver = self
                .receiver
                .lock()
                .map_err(|_| io::Error::other("editor readiness queue is poisoned"))?;
            let mut frames = Vec::new();
            loop {
                match receiver.try_recv() {
                    Ok(frame) => frames.push(frame),
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected)
                        if self.shared.stopped.load(Ordering::Acquire) =>
                    {
                        break;
                    }
                    Err(TryRecvError::Disconnected) => {
                        return Err(io::Error::new(
                            io::ErrorKind::BrokenPipe,
                            "editor readiness worker is not serving",
                        ));
                    }
                }
            }
            if !self.shared.worker_alive.load(Ordering::Acquire)
                && !self.shared.stopped.load(Ordering::Acquire)
            {
                return Err(io::Error::new(
                    io::ErrorKind::BrokenPipe,
                    "editor readiness worker is not serving",
                ));
            }
            Ok(frames)
        }

        pub fn stop(mut self) -> io::Result<()> {
            self.stop_and_join()
        }

        fn stop_and_join(&mut self) -> io::Result<()> {
            self.shared.stopped.store(true, Ordering::Release);
            let _ = wake_session_broker(&self.shared.pipe_name);
            if let Some(worker) = self.worker.take() {
                worker
                    .join()
                    .map_err(|_| io::Error::other("editor readiness thread panicked"))??;
            }
            Ok(())
        }
    }

    impl Drop for EditorReadinessBrokerV1 {
        fn drop(&mut self) {
            let _ = self.stop_and_join();
        }
    }

    fn serve_editor_readiness(
        shared: Arc<EditorReadinessShared>,
        mut pipe: File,
        sender: SyncSender<EditorReadinessFrameV1>,
    ) -> io::Result<()> {
        loop {
            match connect_server_pipe(&pipe) {
                Ok(()) => {}
                Err(_) if shared.stopped.load(Ordering::Acquire) => return Ok(()),
                Err(_) => {
                    drop(pipe);
                    pipe = create_next_server_pipe(&shared.pipe_name)?;
                    continue;
                }
            }
            if shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }

            let mut authenticated = false;
            let mut last_sequence = 0;
            loop {
                let first_byte_deadline = if authenticated {
                    None
                } else {
                    Some(Instant::now() + READINESS_AUTH_TIMEOUT)
                };
                let line = match read_bounded_readiness_line(
                    &mut pipe,
                    &shared.stopped,
                    first_byte_deadline,
                ) {
                    Ok(line) => line,
                    Err(_) if shared.stopped.load(Ordering::Acquire) => return Ok(()),
                    Err(_) => break,
                };
                let frame = match parse_editor_readiness_frame(&line) {
                    Ok(frame) if frame.nonce == shared.expected_nonce => frame,
                    _ if authenticated => {
                        shared.poisoned.store(true, Ordering::Release);
                        return Ok(());
                    }
                    _ => break,
                };
                if authenticated && frame.sequence <= last_sequence {
                    shared.poisoned.store(true, Ordering::Release);
                    return Ok(());
                }
                authenticated = true;
                last_sequence = frame.sequence;
                match sender.try_send(frame) {
                    Ok(()) => {}
                    Err(mpsc::TrySendError::Full(_)) => {
                        shared.poisoned.store(true, Ordering::Release);
                        return Ok(());
                    }
                    Err(mpsc::TrySendError::Disconnected(_)) => return Ok(()),
                }
            }

            if shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            drop(pipe);
            pipe = create_next_server_pipe(&shared.pipe_name)?;
        }
    }

    fn read_bounded_readiness_line(
        pipe: &mut File,
        stopped: &AtomicBool,
        first_byte_deadline: Option<Instant>,
    ) -> io::Result<String> {
        wait_for_pipe_bytes(pipe, stopped, first_byte_deadline)?;
        let frame_deadline = Instant::now() + CONNECTION_READ_TIMEOUT;
        let mut bytes = Vec::with_capacity(128);
        loop {
            let mut byte = [0u8; 1];
            pipe.read_exact(&mut byte)?;
            match byte[0] {
                b'\n' => {
                    return String::from_utf8(bytes).map_err(|_| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "editor readiness frame is not UTF-8",
                        )
                    })
                }
                b'\r' => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "editor readiness frame contains carriage return",
                    ))
                }
                value => {
                    if bytes.len() == MAX_READINESS_FRAME_BYTES {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "editor readiness frame exceeds transport limit",
                        ));
                    }
                    bytes.push(value);
                }
            }
            wait_for_pipe_bytes(pipe, stopped, Some(frame_deadline))?;
        }
    }

    fn wait_for_pipe_bytes(
        pipe: &File,
        stopped: &AtomicBool,
        deadline: Option<Instant>,
    ) -> io::Result<()> {
        loop {
            if stopped.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "editor readiness broker is stopping",
                ));
            }
            let mut available = 0;
            if unsafe {
                PeekNamedPipe(
                    pipe.as_raw_handle().cast(),
                    null_mut(),
                    0,
                    null_mut(),
                    &mut available,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if available > 0 {
                return Ok(());
            }
            if deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "editor readiness client did not complete a frame",
                ));
            }
            thread::sleep(CONNECTION_POLL_INTERVAL);
        }
    }

    pub fn fetch_prepared_request(pipe_name: &OsStr, request_id: &str) -> io::Result<Vec<u8>> {
        validate_request_id(request_id)?;
        let mut pipe = connect_client_pipe(pipe_name)?;
        pipe.write_all(request_id.as_bytes())?;
        pipe.write_all(b"\n")?;
        pipe.flush()?;

        let mut length = [0u8; 4];
        pipe.read_exact(&mut length)?;
        let length = u32::from_le_bytes(length) as usize;
        if length > MAX_PREPARED_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "prepared request exceeds transport limit",
            ));
        }
        let mut wire = vec![0; length];
        pipe.read_exact(&mut wire)?;
        Ok(wire)
    }

    fn serve_session_broker(shared: Arc<SessionBrokerShared>, mut pipe: File) -> io::Result<()> {
        loop {
            match connect_server_pipe(&pipe) {
                Ok(()) => {}
                Err(_) if shared.stopped.load(Ordering::Acquire) => return Ok(()),
                Err(_) => {
                    // A client can connect and close before ConnectNamedPipe
                    // finishes. Recreate the instance instead of terminating
                    // the session worker.
                    drop(pipe);
                    pipe = create_next_server_pipe(&shared.pipe_name)?;
                    continue;
                }
            }
            if shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }

            match wait_for_request_id(&pipe, &shared.stopped) {
                Ok(()) => {}
                Err(_) if shared.stopped.load(Ordering::Acquire) => return Ok(()),
                Err(_) => {
                    // A timeout, early disconnect, or malformed connection is
                    // local to this client. Re-arm the broker for the next one.
                    drop(pipe);
                    pipe = create_next_server_pipe(&shared.pipe_name)?;
                    continue;
                }
            }

            if let Ok(request_id) = read_request_id(&mut pipe) {
                let request = {
                    let mut requests = shared
                        .requests
                        .lock()
                        .map_err(|_| io::Error::other("session broker registry is poisoned"))?;
                    let now = Instant::now();
                    requests.retain(|_, stored| stored.expires_at > now);
                    requests.remove(&request_id).map(|stored| stored.request)
                };
                if let Some(request) = request {
                    // A runner disconnect is scoped to this connection. The
                    // one-shot request stays consumed, but later requests must
                    // continue through the same session broker.
                    let _ = write_prepared_request(&mut pipe, &request);
                }
            }

            if shared.stopped.load(Ordering::Acquire) {
                return Ok(());
            }
            drop(pipe);
            pipe = create_next_server_pipe(&shared.pipe_name)?;
        }
    }

    fn create_next_server_pipe(pipe_name: &str) -> io::Result<File> {
        let deadline = Instant::now() + CONNECTION_READ_TIMEOUT;
        loop {
            match create_server_pipe(pipe_name) {
                Ok(pipe) => return Ok(pipe),
                Err(error)
                    if error.raw_os_error() == Some(ERROR_PIPE_BUSY as i32)
                        && Instant::now() < deadline =>
                {
                    thread::sleep(CONNECTION_POLL_INTERVAL);
                }
                Err(error) => return Err(error),
            }
        }
    }

    fn create_server_pipe(pipe_name: &str) -> io::Result<File> {
        let pipe_name = wide_null(pipe_name);
        let security_descriptor = wide_null(PIPE_DACL);
        let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                security_descriptor.as_ptr(),
                SDDL_REVISION_1,
                &mut descriptor,
                null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attributes = SECURITY_ATTRIBUTES {
            nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor,
            bInheritHandle: 0,
        };
        let handle = unsafe {
            CreateNamedPipeW(
                pipe_name.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
                PIPE_UNLIMITED_INSTANCES,
                PIPE_BUFFER_BYTES,
                PIPE_BUFFER_BYTES,
                PIPE_WAIT_MILLIS,
                &attributes,
            )
        };
        unsafe {
            LocalFree(descriptor);
        }
        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }
        Ok(unsafe { File::from_raw_handle(handle.cast()) })
    }

    fn connect_server_pipe(pipe: &File) -> io::Result<()> {
        let connected = unsafe { ConnectNamedPipe(pipe.as_raw_handle().cast(), null_mut()) };
        if connected == 0 && unsafe { GetLastError() } != ERROR_PIPE_CONNECTED {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    fn connect_client_pipe(pipe_name: &OsStr) -> io::Result<File> {
        connect_client_pipe_with_timeout(pipe_name, PIPE_WAIT_MILLIS)
    }

    fn connect_client_pipe_with_timeout(
        pipe_name: &OsStr,
        timeout_millis: u32,
    ) -> io::Result<File> {
        let pipe_name = wide_null(pipe_name);
        let deadline = Instant::now() + Duration::from_millis(u64::from(timeout_millis));
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let remaining_millis = u32::try_from(remaining.as_millis())
                .unwrap_or(u32::MAX)
                .max(1);
            if unsafe { WaitNamedPipeW(pipe_name.as_ptr(), remaining_millis) } == 0 {
                let error = io::Error::last_os_error();
                if is_transient_pipe_transition(&error) && Instant::now() < deadline {
                    thread::sleep(CONNECTION_POLL_INTERVAL);
                    continue;
                }
                return Err(error);
            }
            let handle = unsafe {
                CreateFileW(
                    pipe_name.as_ptr(),
                    GENERIC_READ | GENERIC_WRITE,
                    0,
                    null(),
                    OPEN_EXISTING,
                    FILE_FLAGS_AND_ATTRIBUTES::default(),
                    null_mut(),
                )
            };
            if handle != INVALID_HANDLE_VALUE {
                return Ok(unsafe { File::from_raw_handle(handle.cast()) });
            }
            let error = io::Error::last_os_error();
            if is_transient_pipe_transition(&error) && Instant::now() < deadline {
                thread::sleep(CONNECTION_POLL_INTERVAL);
                continue;
            }
            return Err(error);
        }
    }

    fn is_transient_pipe_transition(error: &io::Error) -> bool {
        matches!(
            error.raw_os_error(),
            Some(code) if code == ERROR_PIPE_BUSY as i32 || code == ERROR_FILE_NOT_FOUND as i32
        )
    }

    fn wake_session_broker(pipe_name: &str) -> io::Result<()> {
        let mut pipe = connect_client_pipe_with_timeout(OsStr::new(pipe_name), 100)?;
        pipe.write_all(b"00000000000000000000000000000000\n")?;
        pipe.flush()
    }

    fn wait_for_request_id(pipe: &File, stopped: &AtomicBool) -> io::Result<()> {
        let deadline = Instant::now() + CONNECTION_READ_TIMEOUT;
        loop {
            if stopped.load(Ordering::Acquire) {
                return Err(io::Error::new(
                    io::ErrorKind::Interrupted,
                    "session broker is stopping",
                ));
            }
            let mut available = 0;
            if unsafe {
                PeekNamedPipe(
                    pipe.as_raw_handle().cast(),
                    null_mut(),
                    0,
                    null_mut(),
                    &mut available,
                    null_mut(),
                )
            } == 0
            {
                return Err(io::Error::last_os_error());
            }
            if available >= REQUEST_FRAME_BYTES {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "session broker client did not send a request ID",
                ));
            }
            thread::sleep(CONNECTION_POLL_INTERVAL);
        }
    }

    fn write_prepared_request(pipe: &mut File, request: &PreparedRequestV1) -> io::Result<()> {
        let wire = serde_json::to_vec(request).map_err(io::Error::other)?;
        if wire.len() > MAX_PREPARED_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "prepared request exceeds transport limit",
            ));
        }
        pipe.write_all(&(wire.len() as u32).to_le_bytes())?;
        pipe.write_all(&wire)?;
        pipe.flush()
    }

    fn validate_request_id(request_id: &str) -> io::Result<()> {
        if request_id.len() == REQUEST_ID_BYTES
            && request_id.bytes().all(|byte| byte.is_ascii_hexdigit())
        {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "invalid prepared request ID",
            ))
        }
    }

    fn is_valid_nonce(nonce: &str) -> bool {
        nonce.len() == REQUEST_ID_BYTES && nonce.bytes().all(|byte| byte.is_ascii_hexdigit())
    }

    fn read_request_id(pipe: &mut File) -> io::Result<String> {
        let mut bytes = Vec::with_capacity(REQUEST_ID_BYTES);
        loop {
            let mut byte = [0u8; 1];
            pipe.read_exact(&mut byte)?;
            if byte[0] == b'\n' {
                break;
            }
            if bytes.len() == REQUEST_ID_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "prepared request ID exceeds transport limit",
                ));
            }
            bytes.push(byte[0]);
        }
        let request_id = String::from_utf8(bytes)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "request ID is not UTF-8"))?;
        validate_request_id(&request_id)?;
        Ok(request_id)
    }

    fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
        value
            .as_ref()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }
}

#[cfg(windows)]
pub use windows::{
    fetch_prepared_request, parse_editor_readiness_frame, EditorAdapterCapabilityV1,
    EditorLocationKindV1, EditorReadinessBrokerV1, EditorReadinessFrameV1, OneShotBrokerV1,
    SessionBrokerV1, MAX_POWERSHELL_NESTED_DEPTH,
};
