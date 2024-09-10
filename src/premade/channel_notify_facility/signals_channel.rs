pub(super) mod encapsulated;


#[cfg(doc)]
use super::SignalsChannel;
use crate::SignalNumber;
use core::fmt::{self, Debug, Display, Formatter};
extern crate std;
use std::{error::Error, sync::mpsc};


/// A sender side of a channel.  The channel may be either bounded or unbounded.
///
/// If you want to use [`SignalsChannel::install_with_outside_channel`] then the type of the given
/// channel must implement this, which you must do if you give a custom type that doesn't already
/// implement this.  But if you only use [`SignalsChannel::install`] then this trait isn't
/// relevant.
pub trait Sender: Send + Debug + 'static {
    /// Send a signal's number, or representation of that signal, on the channel.  Might block, or
    /// might return the [`SendError::Full`] error if non-blocking is desired, if the channel is
    /// bounded.  If the channel becomes disconnected, calling this will return the
    /// [`SendError::Disconnected`] error and wake up to do so if it was blocked.
    ///
    /// Your implementation may convert the `SignalNumber` to your own type that represents the
    /// signal differently and send that on the channel instead.
    ///
    /// Your implementation may choose to not send anything on the channel.  E.g. if some
    /// particular values of `sig_num` cannot be represented in your type, and it should return
    /// the [`SendError::Ignored`] error in that case.  Or e.g. if the channel is full and sending
    /// would block and you want to avoid that for some reason.
    ///
    /// Note that this blocking is typically desirable so that signals are not unaccounted for
    /// (i.e. not missed) and is typically alright because it's only our internal
    /// "signals-receipt" thread that blocks and signals are always still delivered and counted
    /// anyway and processed later even when that thread was blocked.
    ///
    /// This must remain correct when interrupted by delivery of a signal (like any code that
    /// doesn't mask (block) signals).
    ///
    /// # Errors
    /// If the receiving end of the channel is disconnected.
    fn send(&self, sig_num: SignalNumber) -> Result<(), SendError>;
}


/// Provided for this standard channel type.  Sending never blocks for this type.
///
/// If a `sig_num` value cannot be converted to the chosen `N` type, it won't be sent and the
/// `SendError::Ignored` error will be returned.
impl<N> Sender for mpsc::Sender<N>
where
    SignalNumber: TryInto<N>,
    N: Send + 'static,
{
    #[inline]
    fn send(&self, sig_num: SignalNumber) -> Result<(), SendError> {
        if let Ok(repr) = sig_num.try_into() {
            mpsc::Sender::send(self, repr).or(Err(SendError::Disconnected))
        } else {
            Err(SendError::Ignored)
        }
    }
}

/// Provided for this standard channel type, such that sending blocks, to not miss any signals, if
/// the channel is full.
///
/// If a `sig_num` value cannot be converted to the chosen `N` type, it won't be sent and the
/// `SendError::Ignored` error will be returned.
///
/// (If you'd rather have non-blocking (which could cause signals to be missed) and use
/// `mpsc::SyncSender`, you may have your own wrapper type that `impl`ements `Sender` to use
/// `SyncSender::try_send` instead and that returns `SendError::Full` as appropriate, instead of
/// using this.)
impl<N> Sender for mpsc::SyncSender<N>
where
    SignalNumber: TryInto<N>,
    N: Send + 'static,
{
    #[inline]
    fn send(&self, sig_num: SignalNumber) -> Result<(), SendError> {
        if let Ok(repr) = sig_num.try_into() {
            mpsc::SyncSender::send(self, repr).or(Err(SendError::Disconnected))
        } else {
            Err(SendError::Ignored)
        }
    }
}


/// Error returned by [`Sender::send`] that indicates the way in which the implementer chose to
/// have that operation fail.  With any of these variants, the notification of the signal was not
/// sent.
#[derive(Copy, Clone, Debug)]
#[allow(clippy::exhaustive_enums)]
pub enum SendError {
    /// The channel is disconnected.
    Disconnected,
    /// The channel was full and sending would've blocked.
    Full,
    /// The implementer chose to not send for whatever reason.
    Ignored,
}

impl Display for SendError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            SendError::Disconnected => "signals-notifications channel is disconnected",
            SendError::Full => "signals-notifications channel was full, sending would've blocked",
            SendError::Ignored => "signal wasn't sent, due to implementer choice",
        })
    }
}

impl Error for SendError {}
