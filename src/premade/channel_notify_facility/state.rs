use self::Inner::{Dormant, Installed, Nothing};
use super::{receipts_thread::{self, ReceiptsThread},
            signals_channel::{self, encapsulated::Receiver},
            SignalsChannel, SignalsReceipts};
use crate::SignalNumber;
use core::{fmt::{self, Debug, Display, Formatter},
           mem};
extern crate std;
use std::{error::Error,
          prelude::rust_2021::*,
          sync::{Mutex, MutexGuard}};


/// The global state of the facility's signal handling.  Manages the installing, uninstalling, and
/// finishing of it.
///
/// Only intended to be used by the [`channel_notify_facility`](crate::channel_notify_facility!)
/// macro.
#[derive(Debug)]
pub struct State<C, R>(Mutex<Inner<C, R>>);

/// Whether our handling is or was installed.
#[derive(Default, Debug)]
enum Inner<C, R> {
    /// Our handling is not installed at all.
    #[default]
    Nothing,
    /// Our handling is currently installed.
    Installed {
        receipts_thread: ReceiptsThread<C, R>,
        /// Determines which [`State`] method must be used for uninstalling or finishing.
        is_encapsulated: bool,
    },
    /// Our handling was uninstalled after having been installed.
    Dormant {
        /// The same thread, kept blocked, and held in case our handling is re-installed later.
        /// Re-using the same thread avoids issues that otherwise could occur if a new thread
        /// were created each time our handling is re-installed.
        receipts_thread: ReceiptsThread<C, R>,
    },
}


impl<C: SignalsChannel, R: SignalsReceipts> Inner<C, R> {
    fn do_install<T: signals_channel::Sender>(
        &mut self,
        notify: T,
        is_encapsulated: bool,
    ) -> Result<(), InstallError<T>> {
        // Need a thread to run the processing of the receipts of signals, so that the delegating,
        // to our `ReceiptsThread::handler`, is run in a normal context where it can do whatever
        // (not in the interrupt context of a signal handler which would be extremely limited by
        // async-signal-safety).
        let next = match mem::take(self) {
            // Fresh installing.
            Nothing => Ok(ReceiptsThread::new()?),

            // Re-installing.
            Dormant { receipts_thread } => {
                Ok(if receipts_thread.is_alive() {
                    // Reuse the same thread.
                    receipts_thread
                } else {
                    // Somehow the thread finished outside our control, bizarrely.  This shouldn't
                    // ever happen, but, if this ever does, to be more resilient, we'll create a
                    // new one.
                    drop(receipts_thread);
                    ReceiptsThread::new()? // If early error return, `self` is left as `Nothing`.
                })
            },

            already_installed @ Installed { .. } => Err(already_installed),
        };

        let (inner, result) = match next {
            Ok(receipts_thread) => {
                // Start counting signal deliveries, only after the above succeeds (so that if it
                // errors, the handlers are not installed).  This will reset the global state of
                // `R` (the counters and continue-flag), before the handlers are installed, to
                // start fresh if our handling is being re-installed.  It's alright that our
                // thread isn't ready yet - if any signals are delivered once the handlers are
                // installed but before our thread is ready, those will still be counted, and our
                // thread will still notice and process the receipts of those.
                R::install_all_handlers();

                // Pass the signals-notifications channel to our `ReceiptsThread::control`
                // callback to pass to `ReceiptsThread::handler`, only after the counters were
                // reset (so that the thread won't access them until then).  This makes the thread
                // ready and start its processing.
                receipts_thread.installed(Box::new(notify));

                (Installed { receipts_thread, is_encapsulated }, Ok(()))
            },
            #[rustfmt::skip] // (Avoid bug in rustfmt.)
            Err(already_installed) =>
                (already_installed,
                 Err(InstallError::AlreadyInstalled { unused_notify: notify })),
        };

        *self = inner;
        result
    }

    fn install_with_outside_channel<T: signals_channel::Sender>(
        &mut self,
        notify: T,
    ) -> Result<(), InstallError<T>> {
        self.do_install(notify, false)
    }

    fn install<N>(
        &mut self,
        channel_bound: Option<usize>,
    ) -> Result<Receiver<N, C>, InstallError<()>>
    where
        SignalNumber: TryInto<N>,
        N: Send + 'static,
    {
        let (sender, receiver) = if let Some(bound) = channel_bound {
            signals_channel::encapsulated::bounded(bound)
        } else {
            signals_channel::encapsulated::unbounded()
        };

        Ok(self.do_install(sender, true).map(|()| receiver)?)
    }

    fn do_uninstall(&mut self, expect_encapsulated: bool) -> Result<(), UninstallError> {
        let (inner, result) = match mem::take(self) {
            Installed { receipts_thread, is_encapsulated }
                if is_encapsulated == expect_encapsulated =>
            {
                // Reset the dispositions of the signal numbers to their defaults, and so stop
                // counting signal deliveries.
                R::uninstall_all_handlers();

                let uninstalled = if receipts_thread.is_alive() {
                    // Tell the "signals-receipt" thread to go dormant because our handling has
                    // been uninstalled.
                    receipts_thread.uninstalled();
                    // We don't join the "signals-receipt" thread, in case that thread is blocked
                    // on sending on the channel.  That might only unblock and wake when the
                    // channel is disconnected, which might not be done until after this function
                    // returns.  Trying to join here could deadlock, because of that, so we
                    // don't. Another reason to not join is to not delay the caller.  Instead,
                    // save the thread for later in case our handling is re-installed.
                    Dormant { receipts_thread }
                } else {
                    // Somehow the thread finished outside our control, bizarrely.  This shouldn't
                    // ever happen, but, if this ever does, to be more resilient, we'll adjust for
                    // this.
                    drop(receipts_thread);
                    Nothing
                };

                (uninstalled, Ok(()))
            },

            incongruent @ Installed { .. } => (incongruent, Err(UninstallError::WrongMethod)),

            already_uninstalled @ (Nothing | Dormant { .. }) =>
                (already_uninstalled, Err(UninstallError::AlreadyUninstalled)),
        };

        *self = inner;
        result
    }

    fn uninstall_with_outside_channel(&mut self) -> Result<(), UninstallError> {
        self.do_uninstall(false)
    }

    fn uninstall<N>(&mut self, receiver: Receiver<N, C>) -> Result<(), UninstallError> {
        drop(receiver); // Disconnect the channel, before uninstalling our handling.
        self.do_uninstall(true)
    }

    fn do_finish(&mut self) -> Result<(), FinishError> {
        let result = match mem::take(self) {
            Dormant { receipts_thread } => {
                receipts_thread.finish();
                Ok(())
            },
            Nothing => Err(FinishError::AlreadyFinished),
            #[allow(clippy::unreachable)]
            Installed { .. } => unreachable!(), // Impossible - we just ensured it's uninstalled.
        };
        debug_assert!(matches!(self, Nothing), "is now entirely cleaned-up");
        result
    }

    fn finish_with_outside_channel(&mut self) -> Result<(), FinishError> {
        self.uninstall_with_outside_channel().or_else(Result::from)?;
        self.do_finish()
    }

    fn finish<N>(&mut self, receiver: Receiver<N, C>) -> Result<(), FinishError> {
        self.uninstall(receiver).or_else(Result::from)?;
        self.do_finish()
    }
}


#[doc(hidden)]
impl<C: SignalsChannel, R: SignalsReceipts> State<C, R> {
    #[must_use]
    #[inline]
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self { Self(Mutex::new(Nothing)) }

    fn acquire_inner(&self) -> MutexGuard<'_, Inner<C, R>> {
        #![allow(clippy::expect_used)]
        self.0.lock()
            // Only invalid signal numbers being given by the user, which would cause our other
            // methods to panic, could lead to our mutex becoming poisoned.  Those are given as
            // statically declared, and, once that's known to be correct, this will never panic.
            .expect("mutex should not become poisoned")
    }

    #[must_use]
    #[inline]
    pub fn is_installed(&self) -> bool { matches!(&*self.acquire_inner(), Installed { .. }) }

    #[must_use]
    #[inline]
    pub fn is_dormant(&self) -> bool { matches!(&*self.acquire_inner(), Dormant { .. }) }

    #[must_use]
    #[inline]
    pub fn is_finished(&self) -> bool { matches!(&*self.acquire_inner(), Nothing) }

    #[inline]
    pub fn install<N>(
        &self,
        channel_bound: Option<usize>,
    ) -> Result<Receiver<N, C>, InstallError<()>>
    where
        SignalNumber: TryInto<N>,
        N: Send + 'static,
    {
        self.acquire_inner().install(channel_bound)
    }

    #[inline]
    pub fn install_with_outside_channel<T: signals_channel::Sender>(
        &self,
        notify: T,
    ) -> Result<(), InstallError<T>> {
        self.acquire_inner().install_with_outside_channel(notify)
    }

    #[inline]
    pub fn uninstall<N>(&self, receiver: Receiver<N, C>) -> Result<(), UninstallError> {
        self.acquire_inner().uninstall(receiver)
    }

    #[inline]
    pub fn uninstall_with_outside_channel(&self) -> Result<(), UninstallError> {
        self.acquire_inner().uninstall_with_outside_channel()
    }

    #[inline]
    pub fn finish<N>(&self, receiver: Receiver<N, C>) -> Result<(), FinishError> {
        self.acquire_inner().finish(receiver)
    }

    #[inline]
    pub fn finish_with_outside_channel(&self) -> Result<(), FinishError> {
        self.acquire_inner().finish_with_outside_channel()
    }
}


/// Error returned by [`SignalsChannel::install`] and
/// [`SignalsChannel::install_with_outside_channel`].
#[non_exhaustive]
#[derive(Debug)]
pub enum InstallError<T> {
    /// The signal handling that the `SignalsChannel` manages is already installed.
    AlreadyInstalled {
        /// For [`SignalsChannel::install_with_outside_channel`], the `notify` argument that
        /// wasn't used.  For [`SignalsChannel::install`], this is just `()`.
        unused_notify: T,
    },
    /// The internal "signals-receipt" thread failed to be created, which is unusual.  The exact
    /// possible causes of this are not guaranteed as stable, but the cause can still be accessed
    /// via [`Error::source`].
    ThreadCreateFailed(Box<dyn Error + Send + Sync>),
}

impl<T> From<receipts_thread::NewError> for InstallError<T> {
    #[allow(clippy::missing_inline_in_public_items)]
    fn from(value: receipts_thread::NewError) -> Self {
        Self::ThreadCreateFailed(Box::new(value))
    }
}

impl<T: signals_channel::Sender> From<InstallError<T>> for InstallError<()> {
    #[allow(clippy::missing_inline_in_public_items)]
    fn from(value: InstallError<T>) -> Self {
        match value {
            InstallError::AlreadyInstalled { .. } => Self::AlreadyInstalled { unused_notify: () },
            InstallError::ThreadCreateFailed(e) => Self::ThreadCreateFailed(e),
        }
    }
}

impl<T> Display for InstallError<T> {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            Self::AlreadyInstalled { .. } => "already installed signal handling",
            Self::ThreadCreateFailed(_) => "failed to create internal thread",
        })
    }
}

impl<T: Debug> Error for InstallError<T> {
    #[inline]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::AlreadyInstalled { .. } => None,
            Self::ThreadCreateFailed(e) => Some(&**e),
        }
    }
}


/// Error returned by [`SignalsChannel::uninstall`] and
/// [`SignalsChannel::uninstall_with_outside_channel`].
#[non_exhaustive]
#[derive(Debug)]
pub enum UninstallError {
    /// The signal handling that the `SignalsChannel` manages is already uninstalled.
    AlreadyUninstalled,
    /// The called uninstalling method is incongruent with the method that was used for the
    /// installing.
    WrongMethod,
}

impl Display for UninstallError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            Self::AlreadyUninstalled => "already uninstalled signal handling",
            Self::WrongMethod => "uninstall method wrong for how installed",
        })
    }
}

impl Error for UninstallError {}


/// Error returned by [`SignalsChannel::finish`] and
/// [`SignalsChannel::finish_with_outside_channel`].
#[non_exhaustive]
#[derive(Debug)]
pub enum FinishError {
    /// The signal handling that the `SignalsChannel` manages is already finished.
    AlreadyFinished,
    /// The called finishing method is incongruent with the method that was used for the
    /// installing.
    WrongMethod,
}

impl From<UninstallError> for Result<(), FinishError> {
    #[inline]
    fn from(value: UninstallError) -> Self {
        match value {
            UninstallError::AlreadyUninstalled => Ok(()),
            UninstallError::WrongMethod => Err(FinishError::WrongMethod),
        }
    }
}

impl Display for FinishError {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            Self::AlreadyFinished => "already finished signal handling",
            Self::WrongMethod => "finish method wrong for how installed",
        })
    }
}

impl Error for FinishError {}
