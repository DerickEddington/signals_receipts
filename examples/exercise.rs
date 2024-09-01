//! A more involved example that shows exercising more of the possibilities.

#![allow(
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::panic,
    clippy::print_stdout,
    clippy::unreachable,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]


signals_receipts::premade! {
    (use crate::{set_alarm, unset_alarm};
     use core::{num::Saturating, ops::ControlFlow};
     use std::process::exit;

     #[derive(Default)]
     pub(crate) struct State {
         alarm_count: Saturating<u64>,
         hyper_count: Saturating<u128>,
     }
    )

    type Continue = delegates::State;
    type Break = String;

    // These functions are executed in the separate normal thread, *not* in an async-signal
    // handler.  As such, they can use things like normal, i.e. not be limited by
    // async-signal-safety.

    {callback} => |state| {
        // println!("Loop Iteration ({} alarm, {} urg)", state.alarm_count, state.hyper_count);
        ControlFlow::Continue(state)
    };
    SIGALRM => |receipt| {
        println!("Alarm ({})", receipt.cur_count);
        set_alarm();  // An alarm only triggers once, so reset each time.
        receipt.update_state(|s| s.alarm_count += 1);
    };
    SIGINT => |receipt| {
        println!("Interrupt ({})", receipt.cur_count);
        unset_alarm();
    };
    SIGHUP => |receipt| {
        println!("Reload (Hangup) ({})", receipt.cur_count);
        set_alarm(); // Undo the unset that SIGINT does. Like a config reload.
    };
    SIGUSR1 => |receipt| {
        println!("User-1 ({})", receipt.cur_count);
    };
    SIGURG => |receipt| receipt.get_state_mut().hyper_count += 1;
    SIGQUIT => |receipt| {
        let state = receipt.take_state();
        receipt.break_loop_with(format!("Done (had: {} alarm, {} urg)",
                                        state.alarm_count, state.hyper_count));
    };
    SIGTERM => |receipt| {
        println!("Terminate ({})", receipt.cur_count);
        exit(42);
    };
    SIGABRT => |receipt| panic!("Abort ({})", receipt.cur_count);
}

fn set_alarm() {
    #![allow(unsafe_code)]
    const SECS: core::ffi::c_uint = 2;
    // SAFETY: The argument is proper.
    let prev_remaining = unsafe { libc::alarm(SECS) };
    assert_eq!(0, prev_remaining);
}

fn unset_alarm() {
    #![allow(unsafe_code)]
    // SAFETY: The argument is proper.
    unsafe {
        libc::alarm(0);
    }
}


fn main() {
    match &std::env::args().collect::<Vec<_>>()[..] {
        // No arguments means: parent-process mode.
        [exec_filename] => primary(exec_filename),
        // A PID argument means: child-process mode
        [_, primary_pid] => hyper(primary_pid.parse().expect("argument must be valid")),
        _ => panic!("command-line arguments must be valid"),
    }
}


/// Parent process that handles received signals, which you should manually send to explore what
/// this example does, and which are automatically sent by the child process and by this parent
/// process.
fn primary(exec_filename: &str) {
    use crate::signals_receipts_premade::SignalsReceipts;
    use signals_receipts::{util::mask_all_signals_of_current_thread, Premade as _};
    use std::{os::unix::process::{CommandExt as _, ExitStatusExt as _},
              process::{self, Command},
              thread};

    // Install signal handlers ASAP, in case any signals are delivered immediately.  It's fine
    // that the consuming thread (named "signals") isn't running yet, because any receipts will
    // still be counted and the consuming thread will process those later after it's started.
    // Alternatively, this could be done in the "signals" thread before the consume loop.
    SignalsReceipts::install_all_handlers();

    println!("Send SIGQUIT (key-press ^\\) to exit.");

    // Start the thread that processes receipts of signals.
    let consumer = thread::Builder::new()
        .name("signals".to_owned())
        .spawn(SignalsReceipts::consume_loop)
        .unwrap();

    // Start threads to handle delivery of signals, because the main thread won't be.  Have
    // multiple to try to keep up with how fast the hyper child process sends them.  This amount
    // was chosen experimentally, to drive the "signals" thread to run more.
    for i in 0 .. available_logical_processors() {
        thread::Builder::new()
            .name(format!("worker-{i}"))
            .spawn(|| {
                thread::park(); // This is interrupted to handle the signals, but remains blocked.
                unreachable!();
            })
            .unwrap();
    }

    // Prevent the main thread from handling our signals (but exceptional signal numbers are not
    // masked by this call), so that it can more quickly see when the `consumer.join()` is ready
    // (otherwise, the syscall involved in the `join()` would continually be interrupted by the
    // many delivered signals, which would significantly delay the syscall being able to finish).
    // This must only be done after starting the above threads (otherwise they'd inherit this
    // masking also).
    mask_all_signals_of_current_thread();

    // Just something to exercise sending a signal periodically.
    set_alarm();

    // Start the child process that sends us very many signals endlessly.
    let mut hyper_child = Command::new(exec_filename)
        .arg0("hyper-child")
        .arg(process::id().to_string())
        .spawn()
        .unwrap();

    // Wait for the "signals" thread to finish, which only happens if its loop was broken-out-of
    // by one of the signal-consumer functions, which in our case only happens if SIGQUIT was
    // received.
    let val = consumer.join(); // (If this were interrupted, it'd remain blocked.)

    // Only reached when SIGQUIT was received.

    // Ensure SIGALRM won't be generated and so won't terminate our process after the handlers are
    // uninstalled.
    unset_alarm();

    // Uninstall the signal handlers ASAP after the consume loop no longer exists to process
    // signals.  Alternatively, this could be done in the "signals" thread after the consume loop.
    SignalsReceipts::uninstall_all_handlers();

    // The break-out-of-loop signal-consumer function supplies this value, which we use as our
    // final message.
    println!("{}.", val.unwrap());

    // Terminate and clean-up our child process.
    hyper_child.kill().unwrap();
    let exit_status = hyper_child.wait().unwrap();
    assert_eq!(exit_status.signal(), Some(libc::SIGKILL));
    assert!(!exit_status.core_dumped());
}


/// Child process that sends very many signals to the parent, to cause its signal handling to be
/// under significant load while it handles your other signals, to test that your signals are
/// still handled and consumed properly.
fn hyper(primary_pid: u32) {
    use signals_receipts::{util::mask_all_signals_of_current_thread, SignalNumber};
    use std::thread;

    fn send_signal_to_proc(signum: SignalNumber, pid: libc::pid_t) -> bool {
        #![allow(unsafe_code)]
        // SAFETY: The arguments are proper.
        let r = unsafe { libc::kill(pid, signum) };
        if r == 0 {
            true
        } else {
            let errno = errno::errno().0;
            assert_eq!(errno, libc::ESRCH); // The parent no longer exists.
            false
        }
    }

    // Prevent signals sent to the parent process's group from also affecting us as a child
    // process.  E.g. we don't want ^C (SIGINT) or ^\ (SIGQUIT) done from the terminal to the
    // parent to affect us.  This also masks SIGTSTP, which prevents ^Z from stopping us, which is
    // unusual, but for our example this is desired because then we continue to send our signals
    // while the parent is stopped, like some hypothetical non-child process might do; and when
    // the parent is continued (e.g. via shell `fg` command) it should resume processing the
    // signals (which probably became coalesced) that were sent while it was stopped.  (To force
    // this child process to stop, you can manually send SIGSTOP to it; and when `fg` is done to
    // the group, that will also continue this child.)
    mask_all_signals_of_current_thread();

    let primary_pid = primary_pid.try_into().unwrap();

    // SIGURG is chosen because its default disposition is to ignore, which avoids causing the
    // parent process to terminate when this signal continues to be sent by us after the parent
    // uninstalls its handlers.
    let send_loop = move || while send_signal_to_proc(libc::SIGURG, primary_pid) {};

    // Multiple threads to send the signal at a greater rate.  This amount was chosen
    // experimentally, with my 8-core hyper-threaded CPU (2 logical processors per core) to use
    // only a quarter of the cores, to drive the parent's threads to run more.
    let senders = (0 .. available_logical_processors_div(2 * 4))
        .map(|i| thread::Builder::new().name(format!("sender-{i}")).spawn(send_loop).unwrap())
        .collect::<Vec<_>>();

    for t in senders {
        t.join().unwrap();
    }
}


fn available_parallelism() -> usize {
    std::thread::available_parallelism().map(core::num::NonZeroUsize::get).unwrap_or(1)
}

fn available_logical_processors() -> usize { available_parallelism() }

fn available_logical_processors_div(denom: usize) -> usize {
    #[allow(clippy::arithmetic_side_effects, clippy::integer_division)]
    (available_logical_processors() / denom).max(1)
}
