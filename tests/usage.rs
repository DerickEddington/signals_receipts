#![cfg(test)] // Suppress `clippy::tests_outside_test_module`.
#![allow(
    clippy::missing_assert_message,
    clippy::print_stderr,
    clippy::unreachable,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use crate::signals_receipts_premade::SignalsReceipts;
use core::{ops::ControlFlow,
           sync::atomic::{AtomicBool, Ordering::Relaxed},
           time::Duration};
use libc::{SIGINT, SIGTERM, SIGURG, SIGUSR1};
use signals_receipts::{uninstall_handler, Premade as _, Receipt};
use std::thread;

#[path = "help/util.rs"]
mod util;
use util::raise;


static WAS_IMMEDIATE: AtomicBool = AtomicBool::new(false);
static WAS_INTERRUPTED: AtomicBool = AtomicBool::new(false);
static WAS_TERMINATED: AtomicBool = AtomicBool::new(false);


signals_receipts::premade! {
    (use crate::{terminate, WAS_IMMEDIATE, WAS_INTERRUPTED};
     use core::{ops::ControlFlow, sync::atomic::Ordering::Relaxed};)

    {callback} => |()| {
        eprintln!("Callback");
        ControlFlow::Continue(())
    };
    SIGUSR1 => |_| WAS_IMMEDIATE.store(true, Relaxed);
    SIGINT => |receipt| {
        assert_eq!(receipt.sig_num, libc::SIGINT);
        assert!(receipt.cur_count >= 1);
        eprintln!("Interrupted ({})", receipt.cur_count);
        WAS_INTERRUPTED.store(true, Relaxed);
    };
    SIGURG => |_| unreachable!();
    SIGTERM => terminate;
}

fn terminate(receipt: &mut Receipt<u64>) {
    assert_eq!(receipt.sig_num, SIGTERM);
    assert!(receipt.cur_count >= 1);

    eprintln!("Terminated ({})", receipt.cur_count);
    WAS_TERMINATED.store(true, Relaxed);

    // Cause `signals_receipts::consume_loop()` to return, and so cause the
    // "consume-signals-receipts" thread to finish.
    receipt.flow = ControlFlow::Break(());
}


#[test]
fn main() {
    SignalsReceipts::install_all_handlers();

    // Make the signal handler increment the counter, before the semaphore and consuming thread
    // are ready.  The thread will still detect and consume this.
    raise(SIGUSR1);
    thread::sleep(Duration::from_secs(1));

    let consumer = thread::Builder::new()
        .name("consume-signals-receipts".to_owned())
        .spawn(SignalsReceipts::consume_loop)
        .unwrap();

    // (It can be interesting to increase the amounts of these iterations, when this test is run
    // with --show-output).
    for _ in 0 .. 1 {
        raise(SIGINT);
    }
    uninstall_handler::<SIGURG>(); // Otherwise the `unreachable!()` would fail.
    for _ in 0 .. 10 {
        raise(SIGURG);
    }
    for _ in 0 .. 1 {
        raise(SIGTERM);
    }

    consumer.join().unwrap();

    assert!(WAS_IMMEDIATE.load(Relaxed));
    assert!(WAS_INTERRUPTED.load(Relaxed));
    assert!(WAS_TERMINATED.load(Relaxed));
}
