//! An example of what a child process must do to reset its signal mask, before its program even
//! starts executing, when its parent is blocking signals.  This technique is sometimes necessary
//! when running programs, as subprocesses, that might need signals to be able to be delivered
//! from the very start.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::items_after_statements,
    clippy::missing_assert_message,
    clippy::std_instead_of_core,
    clippy::panic,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

// SIGURG is chosen because its default disposition is to ignore, which avoids causing the
// child process to terminate when this signal is sent by the parent before the child
// installs its handler.
use libc::SIGURG;

#[path = "../tests/help/util.rs"]
mod util;


fn main() {
    let args = std::env::args().collect::<Vec<_>>();
    let args = &args.iter().map(String::as_str).collect::<Vec<&str>>()[..];
    match args {
        // No arguments means: parent-process mode.
        [exec_filename] => parent(exec_filename),
        // An argument means: child-process mode
        [_, "child"] => child(),
        _ => panic!("command-line arguments must be valid"),
    }
}


/// Parent process that masks (blocks) "all" signals but expects its subprocess to be able to
/// receive signals.
fn parent(exec_filename: &str) {
    use signals_receipts::util::{mask_all_signals_of_current_thread,
                                 unmask_all_signals_of_current_thread};
    use std::{io,
              os::unix::process::CommandExt as _,
              process::Command,
              thread,
              time::{Duration, Instant}};
    use util::send_signal_to_proc;

    // Inherited by the child until it resets its mask.
    mask_all_signals_of_current_thread();

    let mut child = Command::new(exec_filename);
    child.arg("child");

    #[allow(clippy::unnecessary_wraps)]
    fn reset_signals_mask() -> io::Result<()> {
        unmask_all_signals_of_current_thread();
        Ok(())
    }
    #[allow(unsafe_code)]
    // SAFETY: `reset_signals_mask` is async-signal-safe (as required by `fork()`), doesn't access
    // any of the parent's resources, and cannot panic.
    unsafe {
        child.pre_exec(reset_signals_mask);
    }

    let mut child = child.spawn().unwrap();
    let child_id = child.id().try_into().unwrap();

    // Send a signal to our child process, and wait for it to finish, within a reasonable
    // deadline.
    let exit_status = {
        let limit = Duration::from_secs(5);
        let deadline = Instant::now() + limit;
        loop {
            // If the child didn't reset its mask, it wouldn't get this.  Retry sending on each
            // loop iteration, in case the child hasn't installed its handler yet, because that's
            // a race between us and them.
            send_signal_to_proc(SIGURG, child_id);

            if Instant::now() < deadline {
                match child.try_wait() {
                    Ok(Some(exit_status)) => break Ok(exit_status),
                    Ok(None) => thread::sleep(Duration::from_millis(250)),
                    Err(e) => break Err(format!("Failed to `child.try_wait()`: {e}")),
                }
            } else {
                break Err(format!("Child failed to exit within deadline {limit:?}"));
            }
        }
    }
    .unwrap_or_else(|e| {
        child.kill().ok();
        panic!("{e}");
    });

    // Check that our child process got the signal.
    assert!(exit_status.success());
}


/// Child process that needs to be able to receive signals immediately.
fn child() {
    use signals_receipts::{install_handler, Semaphore, SemaphoreMethods as _, SemaphoreRef,
                           SignalReceipt};
    use std::{io, pin::Pin, sync::atomic::AtomicU32};

    // This doesn't use the `premade!` macro, so this example doesn't require any package
    // features.

    struct Direct;

    fn semaphore() -> Pin<&'static Semaphore> {
        static SEMAPHORE: Semaphore = Semaphore::uninit();
        Pin::static_ref(&SEMAPHORE)
    }

    impl SignalReceipt<SIGURG> for Direct {
        type AtomicUInt = AtomicU32;

        fn counter() -> &'static Self::AtomicUInt {
            static COUNTER: AtomicU32 = AtomicU32::new(0);
            &COUNTER
        }

        fn semaphore() -> Option<SemaphoreRef<'static>> { semaphore().sem_ref().ok() }
    }


    let sem = semaphore().init().unwrap();

    install_handler::<SIGURG, Direct>(true, false);

    // Wait until the signal is delivered.
    while let Err(()) = sem.wait() {
        assert_eq!(io::ErrorKind::Interrupted, io::Error::last_os_error().kind());
    }

    let count = Direct::take_count();
    assert!(count >= 1);
}
