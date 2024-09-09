#![cfg(test)] // Suppress `clippy::tests_outside_test_module`.
#![allow(
    clippy::print_stdout,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use core::time::Duration;
use libc::{SIGINT, SIGUSR2};
use std::thread;

#[path = "help/util.rs"]
mod util;
use util::raise;


#[test]
fn main() {
    mod weird {
        use signals_receipts::premade;

        premade! {
            pub mod one {
                SIGINT => signals_receipts::Receipt::break_loop;
            }
        }

        premade! {
            pub mod two {
                SIGUSR2 => |_| println!("SIGUSR2");
            }
        }
    }

    use signals_receipts::Premade as _;

    let consumer1 = thread::spawn(weird::one::SignalsReceipts::consume_loop);
    let _consumer2 = thread::spawn(weird::two::SignalsReceipts::consume_loop);

    thread::sleep(Duration::from_secs(2));

    weird::one::SignalsReceipts::install_all_handlers();
    weird::two::SignalsReceipts::install_all_handlers();

    thread::spawn(|| {
        raise(SIGUSR2);
        thread::sleep(Duration::from_secs(1));
        raise(SIGINT);
    });

    consumer1.join().unwrap();
}
