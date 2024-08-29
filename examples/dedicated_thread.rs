//! An example of masking signals for all other threads and having only a single thread be
//! dedicated to doing both the async-delivery handling and the receipt processing.

#![allow(
    clippy::expect_used,
    clippy::missing_assert_message,
    clippy::print_stdout,
    clippy::redundant_closure_for_method_calls,
    clippy::unreachable,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use signals_receipts::util::mask_all_signals_of_current_thread;
use std::thread;


fn main() {
    signals_receipts::premade! {
        SIGINT => |_| {
            use std::io::{stdout, Write as _};
            print!(".");
            stdout().flush().ok();
        };
        SIGQUIT => |control| control.break_loop();
    }
    use signals_receipts::Premade as _;
    use signals_receipts_premade::SignalsReceipts;

    // Mask "all" signals for the main thread and all other threads started hereafter, so that
    // signal handlers are not called on them.  Except, the consume-loop thread, started after,
    // still won't have signals masked because we use the "no_mask" option.
    mask_all_signals_of_current_thread();

    // This also disables `SA_RESTART`, so our "dont-interrupt" thread is properly tested.
    SignalsReceipts::install_all_handlers_with(true, false);

    // Not masking signals for this thread allows it to have signal handlers called on it.  This
    // thread is the only one that can have signal handlers called on it.
    let consumer = thread::spawn(SignalsReceipts::consume_loop_no_mask);

    thread::Builder::new()
        .name("dont-interrupt".to_owned())
        .spawn(|| {
            use core::pin::pin;
            use sem_safe::{non_named::Semaphore as _, plaster::non_named::Semaphore};

            // Just to have something to block on that would error with EINTR if a signal delivery
            // interrupted.
            let sem = pin!(Semaphore::uninit());
            let sem = sem.as_ref().init().unwrap();
            // Because no signal handlers should be called on this thread, this should just block
            // forever.
            let r = sem.wait();
            // If a signal handler were called on this thread, the above would return.
            assert!(r.is_err());
            assert_eq!(libc::EINTR, errno::errno().0);
            // The test is that this will not occur.
            unreachable!();
        })
        .unwrap();

    println!("Key-press ^C (or send SIGINT) many times to print many `.`s,");
    println!("or ^\\ (SIGQUIT) to quit.");

    consumer.join().unwrap();
}
