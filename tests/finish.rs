#![cfg(test)] // Suppress `clippy::tests_outside_test_module`.
#![allow(
    clippy::unreachable,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use core::pin::Pin;
use libc::{SIGQUIT, SIGURG, SIGUSR1};
use sem_safe::{non_named::Semaphore as _, plaster::non_named::Semaphore};
use signals_receipts::Premade as _;
use signals_receipts_premade::SignalsReceipts;
use std::thread;

#[path = "help/util.rs"]
mod util;
use util::raise;


signals_receipts::premade! {
    type Continue = [SemaphoreRef<'static>; 2];
    type Break = &'static str;

    // This order of declaration of these determines the order of their execution within the same
    // iteration of consume_loop.  This order is essential to this test.

    SIGQUIT => |receipt| {
        let [quitter_sem, finisher_sem] = receipt.get_state_ref();
        // Tell the finisher_thread to do the finish operation.
        finisher_sem.post().unwrap();
        // Wait until the finisher_thread indicates it did.  This blocks the receipts_thread, to
        // prevent it from running the next receipt consumer.  Once this wakes, the `continue_flag
        // == false` which will cause the receipts_thread to immediately finish.
        quitter_sem.wait().unwrap();
    };
    SIGUSR1 => |_| unreachable!(); // The early finish prevents running this.
    SIGURG => |_| unreachable!(); // The handlers being uninstalled prevents running this.
}


#[test]
fn main() {
    SignalsReceipts::install_all_handlers();

    // Ensure both of these signals have been delivered and counted already, to ensure the
    // consume_loop will process them in the same loop iteration, to exercise consume_loop's logic
    // that checks the continue_flag between calling each consumer delegate.
    {
        // Only our unusual early finishing prevents this from causing `unreachable!()`.
        raise(SIGUSR1);
        // Cause our unusual early finishing.
        raise(SIGQUIT);
    }

    let [quitter_sem, finisher_sem] = {
        static SEMAPHORES: [Semaphore; 2] = [Semaphore::uninit(), Semaphore::uninit()];
        [&SEMAPHORES[0], &SEMAPHORES[1]].map(|s| Pin::static_ref(s).init().unwrap())
    };

    let receipts_thread = thread::spawn(move || {
        SignalsReceipts::consume_loop_with(false, [quitter_sem, finisher_sem], "finished")
    });

    let finisher_thread = thread::spawn(move || {
        // Wait to be told to do the finish operation.
        finisher_sem.wait().unwrap();
        SignalsReceipts::finish();
        // With the handlers now uninstalled, this will just be ignored.
        raise(SIGURG);
        // Tell the receipts_thread that the finish operation was done.
        quitter_sem.post().unwrap();
    });

    let v = receipts_thread.join().unwrap();
    assert_eq!(v, "finished");

    finisher_thread.join().unwrap();

    // With the handlers now uninstalled, this will just be ignored.
    raise(SIGURG);
}
