//! Child-process lifetime management.
//!
//! On Windows the launcher assigns itself to a job object with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. Every child process spawned afterwards
//! inherits the job, so the OS terminates all of them when the launcher exits
//! or crashes.
//!
//! On Unix (Linux/macOS) the launcher moves itself into a dedicated process
//! group and forks a tiny watchdog process. The watchdog waits for the launcher
//! to close its end of an anonymous pipe (happens on exit or crash) and then
//! kills the entire process group, cleaning up any spawned children.

#[cfg(windows)]
pub fn setup() {
	use std::sync::OnceLock;
	use windows::Win32::Foundation::{CloseHandle, HANDLE};

	struct SafeHandle(HANDLE);
	unsafe impl Send for SafeHandle {}
	unsafe impl Sync for SafeHandle {}

	static JOB_OBJECT: OnceLock<SafeHandle> = OnceLock::new();

	if JOB_OBJECT.get().is_some() {
		return;
	}

	unsafe {
		use windows::Win32::System::JobObjects::{
			AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
			SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
			JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
		};
		use windows::Win32::System::Threading::GetCurrentProcess;
		use windows::core::PCWSTR;

		let job = match CreateJobObjectW(None, PCWSTR::null()) {
			Ok(job) => job,
			Err(_) => {
				eprintln!("Failed to create job object for child cleanup");
				return;
			}
		};

		let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
		info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;

		if SetInformationJobObject(
			job,
			JobObjectExtendedLimitInformation,
			&mut info as *mut _ as *mut _,
			std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
		)
		.is_err()
		{
			eprintln!("Failed to configure job object for child cleanup");
			CloseHandle(job).ok();
			return;
		}

		let current = GetCurrentProcess();
		if AssignProcessToJobObject(job, current).is_err() {
			eprintln!("Failed to assign launcher to job object; child cleanup may not work");
			CloseHandle(job).ok();
			return;
		}

		JOB_OBJECT.set(SafeHandle(job)).ok();
	}
}

#[cfg(unix)]
pub fn setup() {
	use std::sync::OnceLock;

	static WATCHDOG_PIPE_WRITE: OnceLock<libc::c_int> = OnceLock::new();

	if WATCHDOG_PIPE_WRITE.get().is_some() {
		return;
	}

	unsafe {
		// Ensure the launcher is a process group leader. Child processes spawned
		// afterwards inherit this process group by default, so the watchdog can
		// clean them all up with a single group kill.
		if libc::setpgid(0, 0) != 0 {
			eprintln!("Failed to create process group for child cleanup");
			return;
		}

		let mut pipe_fds: [libc::c_int; 2] = [0; 2];
		if libc::pipe(pipe_fds.as_mut_ptr()) != 0 {
			eprintln!("Failed to create watchdog pipe for child cleanup");
			return;
		}

		let read_fd = pipe_fds[0];
		let write_fd = pipe_fds[1];

		// Child processes must not inherit the write end, or the watchdog would
		// never see EOF when the launcher exits.
		if libc::fcntl(write_fd, libc::F_SETFD, libc::FD_CLOEXEC) == -1 {
			eprintln!("Failed to set watchdog pipe close-on-exec");
			libc::close(read_fd);
			libc::close(write_fd);
			return;
		}

		match libc::fork() {
			-1 => {
				eprintln!("Failed to spawn child cleanup watchdog");
				libc::close(read_fd);
				libc::close(write_fd);
			}
			0 => {
				// Watchdog process: wait for the launcher to close the pipe,
				// then kill every process in the shared process group.
				libc::close(write_fd);

				let mut buf: [u8; 1] = [0];
				while libc::read(read_fd, buf.as_mut_ptr().cast(), 1) > 0 {}
				libc::close(read_fd);

				let pgid = libc::getpgrp();
				libc::kill(-pgid, libc::SIGKILL);
				libc::_exit(0);
			}
			_ => {
				// Launcher process: keep the write end open until exit.
				libc::close(read_fd);
				WATCHDOG_PIPE_WRITE.set(write_fd).ok();
			}
		}
	}
}

#[cfg(not(any(windows, unix)))]
pub fn setup() {
	// No OS-level crash cleanup is implemented for this platform yet.
	// Child processes spawned through the frontend are still killed explicitly
	// by the UI on normal shutdown.
}
