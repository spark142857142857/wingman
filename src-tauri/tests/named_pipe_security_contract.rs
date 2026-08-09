use std::io;
use std::os::windows::io::AsRawHandle;
use std::ptr::null_mut;
use uuid::Uuid;
use windows_sys::Win32::Foundation::{LocalFree, ERROR_SUCCESS};
use windows_sys::Win32::Security::Authorization::{
    ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
    SE_KERNEL_OBJECT,
};
use windows_sys::Win32::Security::{DACL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR};
use wingman_lib::interpreter::{PreparedRequestKindV1, PreparedRequestV1};
use wingman_lib::transport::OneShotBrokerV1;

#[test]
fn broker_pipe_dacl_is_protected_for_system_and_its_owner() {
    let pipe_name = format!(
        r"\\.\pipe\wingman-security-test-{}-{}",
        std::process::id(),
        Uuid::new_v4().as_simple()
    );
    let broker = OneShotBrokerV1::bind(
        &pipe_name,
        Uuid::new_v4().as_simple().to_string(),
        PreparedRequestV1 {
            protocol: "wingman.run".to_string(),
            version: 1,
            kind: PreparedRequestKindV1::Reject {
                diagnostic: "prepared".to_string(),
                exit_code: 2,
            },
        },
    )
    .expect("bind secure broker pipe");

    let sddl = read_pipe_dacl_sddl(broker.as_raw_handle()).expect("read named-pipe DACL");
    assert!(sddl.starts_with("D:P"), "DACL must be protected: {sddl}");
    assert!(sddl.contains(";;;SY)"), "SYSTEM must retain access: {sddl}");
    assert!(
        sddl.contains(";;;OW)"),
        "the pipe owner must retain access: {sddl}"
    );
    for forbidden in [";;;WD)", ";;;AN)", ";;;BU)", ";;;AU)"] {
        assert!(
            !sddl.contains(forbidden),
            "broad principal {forbidden} must not access the pipe: {sddl}"
        );
    }
}

fn read_pipe_dacl_sddl(handle: std::os::windows::io::RawHandle) -> io::Result<String> {
    let mut descriptor: PSECURITY_DESCRIPTOR = null_mut();
    let result = unsafe {
        GetSecurityInfo(
            handle.cast(),
            SE_KERNEL_OBJECT,
            DACL_SECURITY_INFORMATION,
            null_mut(),
            null_mut(),
            null_mut(),
            null_mut(),
            &mut descriptor,
        )
    };
    if result != ERROR_SUCCESS {
        return Err(io::Error::from_raw_os_error(result as i32));
    }

    let mut sddl = null_mut();
    let mut length = 0;
    let converted = unsafe {
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            DACL_SECURITY_INFORMATION,
            &mut sddl,
            &mut length,
        )
    };
    if converted == 0 {
        unsafe {
            LocalFree(descriptor);
        }
        return Err(io::Error::last_os_error());
    }

    let value = String::from_utf16(unsafe { std::slice::from_raw_parts(sddl, length as usize) })
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "DACL SDDL is not UTF-16"));
    unsafe {
        LocalFree(sddl.cast());
        LocalFree(descriptor);
    }
    value
}
