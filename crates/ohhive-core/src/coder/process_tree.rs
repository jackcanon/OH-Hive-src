//! Command-scoped process ownership, not a sandbox. Unix descendants must remain in their
//! inherited process group. Windows starts suspended, joins a kill-on-close Job, then resumes.
use std::{future::Future, io};
use tokio::process::{Child, Command};

pub(super) struct ReaderTask<T> {
    pub handle: tokio::task::JoinHandle<T>,
}
impl<T: Send + 'static> ReaderTask<T> {
    pub fn new(future: impl Future<Output = T> + Send + 'static) -> Self {
        Self {
            handle: tokio::spawn(future),
        }
    }
}
impl<T> Drop for ReaderTask<T> {
    fn drop(&mut self) {
        self.handle.abort();
    }
}

#[cfg(unix)]
pub(super) struct Tree(i32, std::sync::atomic::AtomicBool);
#[cfg(unix)]
impl Tree {
    pub fn terminate(&self) {
        if self.1.swap(true, std::sync::atomic::Ordering::SeqCst) {
            return;
        }
        // This group was created exclusively for our child before it could execute.
        unsafe {
            libc::kill(-self.0, libc::SIGKILL);
        }
    }
}
#[cfg(unix)]
impl Drop for Tree {
    fn drop(&mut self) {
        self.terminate();
    }
}
#[cfg(unix)]
pub(super) fn spawn(cmd: &mut Command) -> io::Result<(Child, Tree)> {
    cmd.process_group(0).kill_on_drop(true);
    let child = cmd.spawn()?;
    let group = child.id().expect("new child has a PID") as i32;
    Ok((
        child,
        Tree(group, std::sync::atomic::AtomicBool::new(false)),
    ))
}

#[cfg(windows)]
mod windows {
    use super::*;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
    use windows_sys::Win32::{
        Foundation::{HANDLE, INVALID_HANDLE_VALUE},
        System::{
            Diagnostics::ToolHelp::{
                CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD,
                THREADENTRY32,
            },
            JobObjects::{
                AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
                SetInformationJobObject, TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
                JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
            },
            Threading::{OpenThread, ResumeThread, CREATE_SUSPENDED, THREAD_SUSPEND_RESUME},
        },
    };
    pub(crate) struct Tree(OwnedHandle);
    impl Tree {
        pub fn terminate(&self) {
            unsafe {
                TerminateJobObject(self.0.as_raw_handle() as HANDLE, 1);
            }
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            self.terminate();
        }
    }
    pub(crate) fn spawn(cmd: &mut Command) -> io::Result<(Child, Tree)> {
        // No child code runs before assignment, so there is no spawn-before-assignment race.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let tree = Tree(OwnedHandle::from_raw_handle(job));
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as _,
                std::mem::size_of_val(&info) as u32,
            ) == 0
            {
                return Err(io::Error::last_os_error());
            }
            cmd.creation_flags(CREATE_SUSPENDED).kill_on_drop(true);
            let mut child = cmd.spawn()?;
            let pid = child.id().expect("new child has a PID");
            let process = child
                .raw_handle()
                .ok_or_else(|| io::Error::other("missing child handle"))?;
            if AssignProcessToJobObject(job, process as HANDLE) == 0 {
                let error = io::Error::last_os_error();
                let _ = child.start_kill();
                return Err(error);
            }
            // std/Tokio do not expose the initial thread handle. A suspended new process has
            // one initial thread; enumerate it by PID, never resume unrelated threads.
            let snapshot = CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0);
            if snapshot == INVALID_HANDLE_VALUE {
                return Err(io::Error::last_os_error());
            }
            let _snapshot = OwnedHandle::from_raw_handle(snapshot);
            let mut entry: THREADENTRY32 = std::mem::zeroed();
            entry.dwSize = std::mem::size_of_val(&entry) as u32;
            let mut found = Thread32First(snapshot, &mut entry);
            while found != 0 {
                if entry.th32OwnerProcessID == pid {
                    let thread = OpenThread(THREAD_SUSPEND_RESUME, 0, entry.th32ThreadID);
                    if thread.is_null() {
                        return Err(io::Error::last_os_error());
                    }
                    let _thread = OwnedHandle::from_raw_handle(thread);
                    if ResumeThread(thread) == u32::MAX {
                        return Err(io::Error::last_os_error());
                    }
                    return Ok((child, tree));
                }
                found = Thread32Next(snapshot, &mut entry);
            }
            Err(io::Error::other(
                "could not find the suspended command thread",
            ))
        }
    }
}
#[cfg(windows)]
pub(super) use windows::spawn;
