use std::io;
use std::mem::{size_of, zeroed};
use std::ptr::null_mut;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::Security::{
    GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY,
};
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

pub fn current_process_is_elevated() -> io::Result<bool> {
    let mut token: HANDLE = null_mut();
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(io::Error::last_os_error());
    }

    let mut elevation: TOKEN_ELEVATION = unsafe { zeroed() };
    let mut returned_bytes = 0;
    let result = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&mut elevation as *mut TOKEN_ELEVATION).cast(),
            size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned_bytes,
        )
    };
    let error = if result == 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    unsafe {
        CloseHandle(token);
    }

    if let Some(error) = error {
        return Err(error);
    }
    if returned_bytes < size_of::<TOKEN_ELEVATION>() as u32 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "TokenElevation returned a truncated result",
        ));
    }
    Ok(elevation.TokenIsElevated != 0)
}
