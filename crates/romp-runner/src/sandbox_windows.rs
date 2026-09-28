use crate::sandbox::SandboxParams;
use std::path::Path;
use tracing::info;
use windows_sys::Win32::Foundation::{CloseHandle, LocalFree, HANDLE};
use windows_sys::Win32::Security::Authorization::{
    ConvertStringSecurityDescriptorToSecurityDescriptorW, ConvertStringSidToSidW,
    SetNamedSecurityInfoW, SDDL_REVISION_1, SE_FILE_OBJECT,
};
use windows_sys::Win32::Security::{
    GetLengthSid, GetSecurityDescriptorSacl, SetTokenInformation, TokenIntegrityLevel, ACL,
    LABEL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID, SID_AND_ATTRIBUTES,
    TOKEN_ADJUST_DEFAULT, TOKEN_MANDATORY_LABEL, TOKEN_QUERY,
};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicUIRestrictions,
    JobObjectExtendedLimitInformation, SetInformationJobObject, JOBOBJECT_BASIC_UI_RESTRICTIONS,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION, JOB_OBJECT_UILIMIT_DESKTOP,
    JOB_OBJECT_UILIMIT_DISPLAYSETTINGS, JOB_OBJECT_UILIMIT_EXITWINDOWS,
    JOB_OBJECT_UILIMIT_GLOBALATOMS, JOB_OBJECT_UILIMIT_READCLIPBOARD,
    JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS, JOB_OBJECT_UILIMIT_WRITECLIPBOARD,
};
use windows_sys::Win32::System::SystemServices::SE_GROUP_INTEGRITY;
use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

const LOW_INTEGRITY_SID: &str = "S-1-16-4096";
const LOW_INTEGRITY_LABEL: &str = "S:(ML;OICI;NW;;;LW)";

pub fn apply(params: &SandboxParams<'_>) -> anyhow::Result<()> {
    let scratch = crate::archive::scratch_dir(&std::process::id().to_string());
    std::fs::create_dir_all(&scratch)?;
    for dir in [params.save_dir, scratch.as_path()] {
        allow_low_integrity_writes(dir)?;
    }
    confine_to_job()?;
    lower_integrity()?;
    info!("low integrity sandbox enforced");
    Ok(())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain([0]).collect()
}

fn allow_low_integrity_writes(dir: &Path) -> anyhow::Result<()> {
    let mut descriptor: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
    let converted = unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            wide(LOW_INTEGRITY_LABEL).as_ptr(),
            SDDL_REVISION_1,
            &mut descriptor,
            std::ptr::null_mut(),
        )
    };
    if converted == 0 {
        anyhow::bail!("integrity label: {}", std::io::Error::last_os_error());
    }
    let mut present = 0;
    let mut defaulted = 0;
    let mut sacl: *mut ACL = std::ptr::null_mut();
    unsafe { GetSecurityDescriptorSacl(descriptor, &mut present, &mut sacl, &mut defaulted) };
    let path = wide(&dir.to_string_lossy());
    let status = unsafe {
        SetNamedSecurityInfoW(
            path.as_ptr(),
            SE_FILE_OBJECT,
            LABEL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            std::ptr::null(),
            sacl,
        )
    };
    unsafe { LocalFree(descriptor) };
    if status != 0 {
        anyhow::bail!(
            "label {}: {}",
            dir.display(),
            std::io::Error::from_raw_os_error(status as i32)
        );
    }
    Ok(())
}

fn confine_to_job() -> anyhow::Result<()> {
    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        anyhow::bail!("job object: {}", std::io::Error::last_os_error());
    }
    // SAFETY: the limit structs are plain data; zero means no limit for the fields left unset.
    let mut limits: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
    limits.BasicLimitInformation.LimitFlags =
        JOB_OBJECT_LIMIT_ACTIVE_PROCESS | JOB_OBJECT_LIMIT_DIE_ON_UNHANDLED_EXCEPTION;
    limits.BasicLimitInformation.ActiveProcessLimit = 1;
    let ui = JOBOBJECT_BASIC_UI_RESTRICTIONS {
        UIRestrictionsClass: JOB_OBJECT_UILIMIT_DESKTOP
            | JOB_OBJECT_UILIMIT_DISPLAYSETTINGS
            | JOB_OBJECT_UILIMIT_EXITWINDOWS
            | JOB_OBJECT_UILIMIT_GLOBALATOMS
            | JOB_OBJECT_UILIMIT_READCLIPBOARD
            | JOB_OBJECT_UILIMIT_WRITECLIPBOARD
            | JOB_OBJECT_UILIMIT_SYSTEMPARAMETERS,
    };
    let confined = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&limits as *const JOBOBJECT_EXTENDED_LIMIT_INFORMATION).cast(),
            std::mem::size_of_val(&limits) as u32,
        ) != 0
            && SetInformationJobObject(
                job,
                JobObjectBasicUIRestrictions,
                (&ui as *const JOBOBJECT_BASIC_UI_RESTRICTIONS).cast(),
                std::mem::size_of_val(&ui) as u32,
            ) != 0
            && AssignProcessToJobObject(job, GetCurrentProcess()) != 0
    };
    if !confined {
        let err = std::io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        anyhow::bail!("job object: {err}");
    }
    Ok(())
}

fn lower_integrity() -> anyhow::Result<()> {
    let mut token: HANDLE = std::ptr::null_mut();
    if unsafe {
        OpenProcessToken(
            GetCurrentProcess(),
            TOKEN_ADJUST_DEFAULT | TOKEN_QUERY,
            &mut token,
        )
    } == 0
    {
        anyhow::bail!("process token: {}", std::io::Error::last_os_error());
    }
    let mut sid: PSID = std::ptr::null_mut();
    if unsafe { ConvertStringSidToSidW(wide(LOW_INTEGRITY_SID).as_ptr(), &mut sid) } == 0 {
        let err = std::io::Error::last_os_error();
        unsafe { CloseHandle(token) };
        anyhow::bail!("low integrity SID: {err}");
    }
    let label = TOKEN_MANDATORY_LABEL {
        Label: SID_AND_ATTRIBUTES {
            Sid: sid,
            Attributes: SE_GROUP_INTEGRITY as u32,
        },
    };
    let lowered = unsafe {
        SetTokenInformation(
            token,
            TokenIntegrityLevel,
            (&label as *const TOKEN_MANDATORY_LABEL).cast(),
            std::mem::size_of::<TOKEN_MANDATORY_LABEL>() as u32 + GetLengthSid(sid),
        )
    } != 0;
    let err = std::io::Error::last_os_error();
    unsafe {
        LocalFree(sid);
        CloseHandle(token);
    }
    if !lowered {
        anyhow::bail!("lower integrity: {err}");
    }
    Ok(())
}
