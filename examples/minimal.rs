//! A minimal and somewhat realistic example.

#![allow(
    clippy::print_stdout,
    clippy::redundant_closure_for_method_calls,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use crate::signals_receipts_premade::SignalsReceipts;
use core::{sync::atomic::{AtomicBool, AtomicU32,
                          Ordering::{self, Relaxed}},
           time::Duration};
use signals_receipts::Premade as _;
use std::thread;


signals_receipts::premade! {
    (use crate::{CONFIG, WAS_INTERRUPTED, MEM_ORDERING};)

    SIGUSR1 => |_| { CONFIG.fetch_add(1, MEM_ORDERING); }; // Just a mock config change.
    SIGINT => |_| WAS_INTERRUPTED.store(true, MEM_ORDERING);
    SIGTERM => |control| control.break_loop();
}


static CONFIG: AtomicU32 = AtomicU32::new(1); // Just a mock type.
static WAS_INTERRUPTED: AtomicBool = AtomicBool::new(false);
const MEM_ORDERING: Ordering = Relaxed; // Works for this mock example.


fn main() {
    SignalsReceipts::install_all_handlers();
    let consume_receipts_thread = thread::spawn(SignalsReceipts::consume_loop);
    let is_termination_requested = || consume_receipts_thread.is_finished();

    println!("Send SIGINT (key-press ^C), to pretend to cancel current dummy work.");
    println!("Send SIGUSR1, to pretend to reload dummy configuration.");
    println!("Send SIGTERM, to shutdown, when desired.");

    while !is_termination_requested() {
        let config = CONFIG.load(MEM_ORDERING);
        println!("Doing work, with config {config}.");
        if WAS_INTERRUPTED.load(MEM_ORDERING) {
            println!("Cancelling current work, but will keep processing.");
            WAS_INTERRUPTED.store(false, MEM_ORDERING);
        }
        thread::sleep(Duration::from_secs(1));
    }

    println!("Shutdown done.");
}
