//! These are the private items that the `channel_notify_facility!` macro expansion needs
//! fully-`pub` access to from external crates.

pub use crate::premade::channel_notify_facility::{receipts_thread::{DelegatesState,
                                                                    ReceiptsThread},
                                                  state::State,
                                                  SignalsReceipts};
