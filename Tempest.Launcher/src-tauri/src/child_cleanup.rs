//! Child-process lifetime management.
//!
//! On Windows the launcher assigns itself to a job object for tracking, but
//! does NOT set KILL_ON_JOB_CLOSE so game instances (Paladins) survive when
//! the launcher exits. The frontend gracefully cleans up non-game children
//! before closing.
//!
//! On Unix (Linux/macOS) no watchdog is set up for the same reason.

#[cfg(windows)]
pub fn setup() {
	// Job object is intentionally minimal — no KILL_ON_JOB_CLOSE.
	// Game instances spawned by the CLI survive launcher exit.
}

#[cfg(unix)]
pub fn setup() {
	// No watchdog. Game instances survive launcher exit.
}

#[cfg(not(any(windows, unix)))]
pub fn setup() {
}
