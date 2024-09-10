// Any namespace that has all the SIG* signals' names (which varies per OS) as `const`s under it
// would work.
pub use libc as signals_names;

pub trait Sealed {}

#[cfg(feature = "channel_notify_facility")]
pub mod channel_notify_facility;
