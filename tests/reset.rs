#![cfg(test)] // Suppress `clippy::tests_outside_test_module`.
#![allow(
    clippy::arithmetic_side_effects,
    clippy::unreachable,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use cfg_if::cfg_if;
use core::{ffi::c_int, sync::atomic::Ordering::Relaxed};
use signals_receipts::{Premade, SemaphoreMethods as _, SignalNumber, SignalReceipt};
use signals_receipts_premade::SignalsReceipts;
use std::thread;

#[path = "help/util.rs"]
mod util;
use util::raise;


signals_receipts::premade! {
    type Continue = u64;
    type Break = &'static str;

    {callback} => |state| {
        use core::ops::ControlFlow;

        let sem_count = state;
        if sem_count == 0 {
            ControlFlow::Break("broke")
        } else {
            ControlFlow::Continue(sem_count - 1)
        }
    };

    SIGURG => |_| unreachable!();
}

/// Just any one of the signal numbers with default disposition of ignoring.
const SIG: SignalNumber = libc::SIGURG;


fn continue_flag() -> bool { SignalsReceipts::continue_flag().load(Relaxed) }

fn signal_delivery_count() -> u64 {
    <SignalsReceipts as SignalReceipt<SIG>>::counter().load(Relaxed)
}

fn semaphore_count() -> c_int {
    cfg_if! { if #[cfg(not(target_os = "macos"))] {
        <SignalsReceipts as SignalReceipt<SIG>>::semaphore().unwrap().get_value()
    } else {
        unreachable!()
    } }
}

fn assert_values(flag: bool, sig_count: u64, sem_count: c_int) {
    assert_eq!(continue_flag(), flag);
    assert_eq!(signal_delivery_count(), sig_count);
    #[cfg(not(target_os = "macos"))]
    assert_eq!(semaphore_count(), sem_count);
}


#[test]
fn main() {
    SignalsReceipts::install_all_handlers();

    // Don't yet want a consuming thread running `consume_loop`, so the signal's counter is not
    // taken (which would reset it to zero) by that, so we can test its value changes.

    // Initialize the semaphore manually, since we don't do `consume_loop`.
    <SignalsReceipts as Premade>::semaphore().init().unwrap();

    assert_values(true, 0, 0);

    for _ in 0 .. 1_000 {
        raise(SIG);
    }
    assert_values(true, 1_000, 1_000);

    SignalsReceipts::finish();
    assert_values(false, 1_000, 1_001);

    for _ in 0 .. 1_000 {
        raise(SIG); // (This is why its default disposition needs to be ignoring.)
    }
    assert_values(false, 1_000, 1_001);

    // Re-installing the handling resets the counter(s) and the continue-flag.
    SignalsReceipts::install_all_handlers();
    assert_values(true, 0, 1_001);

    // Now have a consuming thread running `consume_loop`.  Since the semaphore already has a
    // positive value, the loop will have to iterate that many times pointlessly.
    let t = thread::spawn(|| {
        let sem_count = 1_001;
        let state = u64::try_from(sem_count).unwrap();
        SignalsReceipts::consume_loop_with(true, state, "unused")
    });

    let v = t.join().unwrap();
    assert_eq!(v, "broke");

    // Check that the loop iterated as many times as the preexisting semaphore value, even though
    // no signals were received.  This is not desirable behavior, but it's just due to there not
    // being a way to forcibly reset the value of a semaphore, and it's harmless.
    assert_values(true, 0, 0);
}
