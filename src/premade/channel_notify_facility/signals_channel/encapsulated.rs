use super::{super::SignalsChannel, SendError};
use crate::SignalNumber;
use core::{fmt::{self, Debug, Formatter},
           marker::PhantomData};
extern crate std;
use std::sync::mpsc;


/// The receiving end of a premade signals-notifications channel that knows its creator.  This is
/// returned by [`SignalsChannel::install`].
///
/// This cannot be cloned, and so is single-owner, as needed to ensure disconnection when
/// giving-up ownership to [`SignalsChannel::uninstall`] or [`SignalsChannel::finish`].
pub struct Receiver<N, C> {
    inner:    mpsc::Receiver<N>,
    _creator: PhantomData<C>,
}

/// Enables users to use `Self` as a receiver.
impl<N, C> AsRef<mpsc::Receiver<N>> for Receiver<N, C> {
    #[inline]
    fn as_ref(&self) -> &mpsc::Receiver<N> { &self.inner }
}

/// Want `Debug` for this but without `N: Debug`.
impl<N, C> Debug for Receiver<N, C> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Receiver").field("inner", &self.inner).finish()
    }
}


/// The corresponding other end of channels with our [`Receiver`] type.  Only used internally to
/// send signals notifications when our handling was installed with [`SignalsChannel::install`].
pub(in super::super) enum Sender<N> {
    Bounded(mpsc::SyncSender<N>),
    Unbounded(mpsc::Sender<N>),
}

impl<N> super::Sender for Sender<N>
where
    SignalNumber: TryInto<N>,
    N: Send + 'static,
{
    fn send(&self, sig_num: SignalNumber) -> Result<(), SendError> {
        if let Ok(repr) = sig_num.try_into() {
            match self {
                Sender::Bounded(s) => s.send(repr),
                Sender::Unbounded(s) => s.send(repr),
            }
            .or(Err(SendError::Disconnected))
        } else {
            Err(SendError::Ignored)
        }
    }
}

/// Want `Debug` for this but without `N: Debug`.
impl<N> Debug for Sender<N> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (name, field): (_, &dyn Debug) = match self {
            Sender::Bounded(s) => ("Bounded", s),
            Sender::Unbounded(s) => ("Unbounded", s),
        };
        f.debug_tuple(name).field(field).finish()
    }
}


/// Creates a new premade signals-notifications channel that is bounded.
pub(in super::super) fn bounded<N, C: SignalsChannel>(bound: usize) -> (Sender<N>, Receiver<N, C>)
where
    SignalNumber: TryInto<N>,
    N: Send + 'static,
{
    let (sender, receiver) = mpsc::sync_channel(bound);
    (Sender::Bounded(sender), Receiver { inner: receiver, _creator: PhantomData })
}

/// Creates a new premade signals-notifications channel that is unbounded.
pub(in super::super) fn unbounded<N, C: SignalsChannel>() -> (Sender<N>, Receiver<N, C>)
where
    SignalNumber: TryInto<N>,
    N: Send + 'static,
{
    let (sender, receiver) = mpsc::channel();
    (Sender::Unbounded(sender), Receiver { inner: receiver, _creator: PhantomData })
}
