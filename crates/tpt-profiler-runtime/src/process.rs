//! Child-process execution with timeout and cancellation (spec §8.2,
//! §50). This is the only place the profiler runs foreign processes, and
//! it is deliberately small: spawn, poll, kill on timeout or cancel,
//! capture output.

use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// Result of one child-process run.
#[derive(Debug, Clone)]
pub struct RunOutcome {
    /// Process exited with status 0.
    pub success: bool,
    /// Exit code, when the process exited normally.
    pub exit_code: Option<i32>,
    /// Wall-clock duration of the run.
    pub wall_time: Duration,
    /// Collected stdout (best-effort, truncated by the OS pipe buffer
    /// if the process ignores draining — output is drained concurrently).
    pub stdout: String,
    /// Collected stderr.
    pub stderr: String,
    /// The run exceeded its timeout and was killed.
    pub timed_out: bool,
    /// The run was cancelled via the cancel flag.
    pub cancelled: bool,
}

/// A shared cancellation flag the CLI can set (e.g. on Ctrl-C).
#[derive(Debug, Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Signals cancellation.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }

    /// Whether cancellation was signalled.
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

/// Poll interval while waiting for the child.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// Runs `command` with a timeout and optional cancellation.
///
/// On timeout or cancellation the child is killed. Note the platform
/// limit (documented, spec §8.2): on Windows `kill()` terminates the
/// child but not necessarily its grandchildren.
pub fn run(
    command: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
) -> std::io::Result<RunOutcome> {
    run_observed(command, timeout, cancel, &mut |_| {})
}

/// Like [`run`], but calls `observe` with the child's process id on every
/// poll while it runs (about every 10 ms). Used to sample the child from
/// outside, for example its resident memory (spec §14). The observer is not
/// called after the child has finished, so it sees only live processes.
///
/// # Errors
/// Fails when the command cannot be spawned.
pub fn run_observed(
    command: &mut Command,
    timeout: Duration,
    cancel: Option<&CancelToken>,
    observe: &mut dyn FnMut(u32),
) -> std::io::Result<RunOutcome> {
    let child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let started = Instant::now();
    let mut child = PipedReader::spawn(child);

    let deadline = started + timeout;
    loop {
        if let Some(token) = cancel {
            if token.is_cancelled() {
                child.kill()?;
                let (stdout, stderr, status) = child.finish()?;
                return Ok(RunOutcome {
                    success: false,
                    exit_code: status.code(),
                    wall_time: started.elapsed(),
                    stdout,
                    stderr,
                    timed_out: false,
                    cancelled: true,
                });
            }
        }
        if let Some((stdout, stderr, status)) = child.try_finish()? {
            return Ok(RunOutcome {
                success: status.success(),
                exit_code: status.code(),
                wall_time: started.elapsed(),
                stdout,
                stderr,
                timed_out: false,
                cancelled: false,
            });
        }
        if Instant::now() >= deadline {
            child.kill()?;
            let (stdout, stderr, status) = child.finish()?;
            return Ok(RunOutcome {
                success: false,
                exit_code: status.code(),
                wall_time: started.elapsed(),
                stdout,
                stderr,
                timed_out: true,
                cancelled: false,
            });
        }
        observe(child.child.id());
        std::thread::sleep(POLL_INTERVAL);
    }
}

/// Owns a child plus background threads draining its output pipes, so a
/// chatty child can never deadlock on a full pipe (spec §8.2).
struct PipedReader {
    child: Child,
    stdout: Option<std::thread::JoinHandle<String>>,
    stderr: Option<std::thread::JoinHandle<String>>,
}

impl PipedReader {
    fn spawn(mut child: Child) -> Self {
        let stdout = child.stdout.take().map(drain);
        let stderr = child.stderr.take().map(drain);
        Self {
            child,
            stdout,
            stderr,
        }
    }

    fn try_finish(
        &mut self,
    ) -> std::io::Result<Option<(String, String, std::process::ExitStatus)>> {
        match self.child.try_wait()? {
            Some(status) => Ok(Some(self.collect(status))),
            None => Ok(None),
        }
    }

    fn finish(&mut self) -> std::io::Result<(String, String, std::process::ExitStatus)> {
        let status = self.child.wait()?;
        Ok(self.collect(status))
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()?;
        Ok(())
    }

    fn collect(
        &mut self,
        status: std::process::ExitStatus,
    ) -> (String, String, std::process::ExitStatus) {
        let stdout = self
            .stdout
            .take()
            .map_or_else(String::new, |handle| handle.join().unwrap_or_default());
        let stderr = self
            .stderr
            .take()
            .map_or_else(String::new, |handle| handle.join().unwrap_or_default());
        (stdout, stderr, status)
    }
}

fn drain<R: Read + Send + 'static>(mut pipe: R) -> std::thread::JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buffer = Vec::new();
        // Read in bounded chunks; a runaway process cannot grow this
        // past a few MiB without the profiler cutting it off elsewhere.
        let mut chunk = [0_u8; 8192];
        while let Ok(read) = pipe.read(&mut chunk) {
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        String::from_utf8_lossy(&buffer).to_string()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn echo_command(text: &str) -> Command {
        #[cfg(windows)]
        {
            let mut command = Command::new("cmd");
            command.args(["/C", &format!("echo {text}")]);
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("echo");
            command.arg(text);
            command
        }
    }

    fn sleeper_command(seconds: u64) -> Command {
        #[cfg(windows)]
        {
            let mut command = Command::new("timeout");
            command.arg(format!("/t {}", seconds.max(1)));
            command.env("TIMEOUT", "1"); // make `timeout` non-interactive
            command
        }
        #[cfg(not(windows))]
        {
            let mut command = Command::new("sleep");
            command.arg(seconds.to_string());
            command
        }
    }

    #[test]
    fn successful_command_returns_output_and_time() {
        let outcome = run(&mut echo_command("hello"), Duration::from_secs(10), None)
            .expect("spawn works on all three OSes");
        assert!(outcome.success);
        assert!(!outcome.timed_out);
        assert!(!outcome.cancelled);
        assert!(outcome.stdout.contains("hello"));
        assert!(outcome.wall_time < Duration::from_secs(10));
    }

    #[test]
    fn failing_command_reports_nonzero_status() {
        #[cfg(windows)]
        let mut command = {
            let mut inner = Command::new("cmd");
            inner.args(["/C", "exit 3"]);
            inner
        };
        #[cfg(not(windows))]
        let mut command = Command::new("false");
        let outcome = run(&mut command, Duration::from_secs(10), None).expect("run");
        assert!(!outcome.success);
        assert_eq!(outcome.exit_code, Some(3));
    }

    #[test]
    fn timeout_kills_the_child() {
        let outcome = run(&mut sleeper_command(30), Duration::from_millis(300), None).expect("run");
        assert!(outcome.timed_out);
        assert!(!outcome.success);
        assert!(
            outcome.wall_time < Duration::from_secs(5),
            "kill must be prompt: {:?}",
            outcome.wall_time
        );
    }

    #[test]
    fn cancellation_kills_the_child() {
        let token = CancelToken::default();
        // Cancel from another thread while the sleeper runs.
        std::thread::scope(|scope| {
            scope.spawn(|| {
                std::thread::sleep(Duration::from_millis(100));
                token.cancel();
            });
            let outcome = run(
                &mut sleeper_command(30),
                Duration::from_secs(30),
                Some(&token),
            )
            .expect("run");
            assert!(outcome.cancelled);
            assert!(!outcome.success);
        });
    }
}
