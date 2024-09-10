use super::{signals_channel, SignalsChannel, SignalsReceipts};
use crate::{help::assert_errno_is_overflow, Receipt, SemaphoreMethods as _, SemaphoreRef};
use core::{fmt::{self, Display, Formatter},
           marker::PhantomData,
           mem::size_of,
           ops::ControlFlow};
extern crate std;
use std::{error::Error, io, prelude::rust_2021::*, sync::mpsc, thread};


/// Internal thread that processes updates to signal-receipt counters and that sends, over a
/// channel, notifications of signals received.
#[derive(Debug)]
pub struct ReceiptsThread<C, R> {
    /// Internal channel to control the thread.  (This isn't the notifications channel.)
    controller:        mpsc::Sender<Control>,
    semaphore:         SemaphoreRef<'static>,
    join_handle:       thread::JoinHandle<()>,
    _signals_channel:  PhantomData<C>,
    _signals_receipts: PhantomData<R>,
}


/// Tells the thread what to do when the user is installing or uninstalling our handling.
enum Control {
    /// The user has invoked installing our signal handling and has provided a channel to send
    /// notifications on.
    Installed {
        /// The channel to send notifications of signals received.
        notify: Box<dyn signals_channel::Sender>,
    },
    /// The user has invoked uninstalling our signal handling.
    Uninstalled,
}


/// What the [`ReceiptsThread::control`] callback and the [`ReceiptsThread::handler`] delegates,
/// which are called on the thread, need to have depending on whether our handling is installed or
/// not.
#[derive(Debug)]
#[allow(private_interfaces, clippy::exhaustive_enums)]
pub enum DelegatesState {
    /// Block the thread until told what to do.  This is the thread's initial state and its state
    /// when our signal handling is uninstalled.
    Dormant {
        /// The internal channel to control the thread.  Same channel as when `Active`.
        controller: mpsc::Receiver<Control>,
    },
    /// The thread's state when installed.
    Active {
        /// The channel to send notifications of signals received.
        notify:     Box<dyn signals_channel::Sender>,
        /// The internal channel to control the thread.  Same channel as when `Dormant`.
        controller: mpsc::Receiver<Control>,
    },
}


impl<C: SignalsChannel, R: SignalsReceipts> ReceiptsThread<C, R> {
    const NAME: &'static str = {
        let name = "signals-receipt";
        if name.len() < {
            // Linux's limit of 16 (including a nul) is the smallest among OSs.
            const LINUX_LIMIT: usize = 16;
            const SMALLEST_LIMIT_AMONG: usize = LINUX_LIMIT;
            SMALLEST_LIMIT_AMONG
        } {
            name
        } else {
            panic!("limited by `pthread_setname_np` or the OS");
        }
    };
    /// The maximum function-call depth and stack-allocation usage of this type of thread is very
    /// small and is statically bounded.  Having this smaller size isn't necessary on OSs
    /// that, like Linux, use over-commit and on-demand paging for the stacks, but since this
    /// crate is intended to be portable to any POSIX OS where that might not be the case, we
    /// configure this smaller size since we know it's all that's ever needed.
    const STACK_SIZE: usize = {
        // The optimizations of `release` build result in less needed than `dev`.  My measurements
        // (on Linux glibc x86_64) needed 6.1 KiB for `dev` and 0.9 KiB for `release` (not
        // counting the TCB and TLS).  The values chosen here are more than enough (even though
        // our dev:release ratio of 2:1 isn't the same as 6.1:0.9).
        let needed_words = if cfg!(debug_assertions) { 2 } else { 1 } * 1024;
        let needed = needed_words * size_of::<usize>();
        4 * needed
    };

    #[allow(clippy::unwrap_in_result)]
    pub(super) fn new() -> Result<Self, NewError> {
        // The internal channel to control the thread.  It's unbounded, so that sending on it will
        // never block, but its amount should stay very small when the user is not pathological.
        let (controller_sender, controller_receiver) = mpsc::channel();

        // Ensure that the thread's semaphore is initialized, so we can keep a reference to it as
        // needed for uninstalling, and so we know it is before proceeding (otherwise the thread
        // would panic quietly, if initializing there failed).  When the semaphore is already
        // initialized (because we're re-installing our handling), it's impossible for this to
        // fail.  When it's not, only a very unusual OS-level failure due to low semaphore limits
        // (similar to limits on open FDs) could cause this to fail.
        let semaphore = {
            let semaphore = R::semaphore();
            semaphore.init().or_else(|is_already| {
                if is_already {
                    // Because nothing outside our facility's control can access the semaphore and
                    // because the internal `State` changes are mutex'ed, there's nothing else
                    // that could be concurrently initializing it, and so it's impossible for this
                    // to panic.
                    #[allow(clippy::expect_used)]
                    Ok(semaphore.sem_ref().expect("init was already completed"))
                } else {
                    Err(NewError::SemaphoreInitFailed(io::Error::last_os_error()))
                }
            })?
        };

        let join_handle = thread::Builder::new()
            .name(Self::NAME.to_owned())
            .stack_size(Self::STACK_SIZE)
            .spawn(Self::main(controller_receiver))
            // Only an OS-level failure to create a thread could cause this to fail.
            .map_err(NewError::ThreadCreateFailed)?;

        Ok(Self {
            controller: controller_sender,
            semaphore,
            join_handle,
            _signals_channel: PhantomData,
            _signals_receipts: PhantomData,
        })
    }

    fn main(controller: mpsc::Receiver<Control>) -> impl FnOnce() + Send + 'static {
        // The closure executed by our spawned thread.
        || {
            // Initially, wait until told to proceed, to ensure that the operation that created
            // this thread has completed its resetting of the global state of `R`.
            let notify = match controller.recv() {
                Ok(Control::Installed { notify }) => notify,
                #[allow(clippy::unreachable)] // It's impossible for this to panic.
                Ok(Control::Uninstalled) | Err(mpsc::RecvError) => unreachable!(),
            };

            let () = R::consume_loop_with(
                // Don't mask signals for this thread.  Allow signals to be delivered to,
                // i.e. have their handlers called on and interrupt, this thread.  This ensures
                // at least this thread is available to handle signal delivery,
                // in case the program masks (blocks) signals in all its other
                // threads.  This is alright because our `Self::handler` &
                // `Self::control` remain correct when interrupted by signal
                // delivery.  (This is separate from, and not needed for, the processing of the
                // receipts of signals done by this thread.  This just allows signal handlers to
                // also use this thread.)
                false,
                // Pass the channels to the loop to pass to our `Self::control` callback and our
                // `Self::handler` delegates.
                DelegatesState::Active { notify, controller },
                (),
            );
        }
    }

    pub(super) fn is_alive(&self) -> bool { !self.join_handle.is_finished() }

    fn send(&self, control_message: Control) {
        #[allow(clippy::expect_used)]
        self.controller
            .send(control_message)
            // We ensure our internal controller channel is always connected, and so it's
            // impossible for this to panic - it's never disconnected (until the thread is
            // finished, at which point we never do sending anymore).
            .expect("controller channel is always connected");
    }

    pub(super) fn installed(&self, notify: Box<dyn signals_channel::Sender>) {
        self.send(Control::Installed { notify });
    }

    pub(super) fn uninstalled(&self) {
        self.send(Control::Uninstalled);
        // Ensure that the "signals-receipt" thread wakes to see our `Uninstalled` message, in
        // case that thread is blocked waiting on the semaphore (which is the most likely case).
        let r = self.semaphore.post();
        // This `.post()` can only fail if the semaphore's value is maxed, in which case the
        // thread is already being woken.
        if r.is_err() {
            #[allow(clippy::unreachable)]
            assert_errno_is_overflow(|| {
                unreachable!(); // Impossible - `sem_safe` ensures the semaphores are valid.
            });
        }
    }

    pub(super) fn finish(self) {
        // Tell the consuming thread to finish.  But the disconnecting we do next might instead be
        // what really causes the thread to finish.  Either way is fine.  It depends on where the
        // thread was at when the immediately-preceding uninstall operation was done.
        R::finish();
        // Disconnect the controller channel, to ensure the thread wakes (because it could be
        // blocked on this channel now), to see that it must finish.
        drop(self.controller);
        // Wait for the thread to finish, only after having dropped our controller.  (It's
        // unnecessary, here, to deal with the possibility that the thread panicked.)
        self.join_handle.join().ok();
    }

    /// Send, on the `notify` channel, notification of receipt of a signal.  This is the delegate
    /// called by [`crate::consume_loop`] on our "signals-receipt" thread when processing receipt
    /// of a signal.
    ///
    /// This might be interrupted by deliveries of signals (like any code that doesn't mask
    /// (block) signals).  This remains correct when that occurs, because
    /// [`signals_channel::Sender::send()`] remains correct - if it's blocking the thread, it'll
    /// continue blocking, or if it's currently sending, it'll continue sending, after being
    /// interrupted.
    #[allow(clippy::missing_inline_in_public_items)]
    pub fn handler(receipt: &mut Receipt<u64, (), DelegatesState>) {
        let receipt = &*receipt; // As immutable.

        let notify = match receipt.get_state_ref() {
            DelegatesState::Active { notify, .. } => notify,
            // Our `Self::control` callback blocks our "signals-receipt" thread until a
            // notifications channel has been provided, before that thread can call us, and so
            // it's impossible for this to panic.
            #[allow(clippy::unreachable)]
            DelegatesState::Dormant { .. } => unreachable!(),
        };

        // If `receipt.cur_count >= 2`, we don't send more than one notification on the channel.
        // This coalesces multiple of the same that were received within the short time span of a
        // single iteration of the consuming loop.  This is deemed acceptable because the OS might
        // already be doing its own coalescing and so you can't rely on that to not happen anyway.

        // It's ok if this blocks waiting to send on the channel.  This honors the capacity of the
        // channel that the user chose to install.  It's the "signals-receipt" thread that might
        // block here, which will delay its processing of any further received signals, and that's
        // desired because we don't want to process them until the channel is ready for more.
        // While blocked here, the receipts of signals will still be counted (because
        // `crate::handler` will still run when a signal is delivered and will still increment
        // their counters), and so the processing of further signals will still be done after we
        // wake up when the channel is ready.
        notify.send(receipt.sig_num).ok();
        // If the send fails (because the channel either: is disconnected, is full and chooses to
        // not block, or chooses to ignore this signal number), we just ignore that.
    }

    /// Assist with transitioning between installed and uninstalled states.  This is the callback
    /// called by [`crate::consume_loop`] on our "signals-receipt" thread before each iteration of
    /// processing receipts of signals.
    ///
    /// This might be interrupted by deliveries of signals (like any code that doesn't mask
    /// (block) signals).  This remains correct when that occurs, because
    /// [`mpsc::Receiver::recv()`] remains correct - if it's blocking the thread, it'll continue
    /// blocking, or if it's currently receiving, it'll continue receiving, after being
    /// interrupted.
    #[allow(clippy::missing_inline_in_public_items, clippy::must_use_candidate)]
    pub fn control(state: DelegatesState) -> ControlFlow<(), DelegatesState> {
        use self::{Control::{Installed, Uninstalled},
                   DelegatesState::{Active, Dormant}};
        use mpsc::{RecvError,
                   TryRecvError::{Disconnected, Empty}};
        use ControlFlow::{Break, Continue};

        match state {
            // Check if there's a new message telling us what to do.  This is the thread's state
            // when installed.
            Active { notify, controller } => match controller.try_recv() {
                // There is not any new message.  No change.  This is the most frequent case.
                Err(Empty) => Continue(Active { notify, controller }),
                // We're being told to go dormant - uninstalling was done.
                Ok(Uninstalled) => {
                    // Disconnect the notifications channel.
                    drop(notify);
                    // Recur to block until re-installed.
                    Self::control(Dormant { controller })
                },
                // Installation of a different notifications channel, to replace the current one.
                // This message while we're in this state, does not occur actually.
                Ok(Installed { notify: new_notify }) => {
                    debug_assert!(false, "doesn't occur with current design");
                    Continue(Active { notify: new_notify, controller })
                },
                // If the controller channel is ever disconnected, that means to finish the
                // thread.
                Err(Disconnected) => Break(()),
            },

            // Block our "signals-receipt" thread until told what to do.  This is the thread's
            // state when our signal handling is uninstalled.
            Dormant { controller } => match controller.recv() {
                // Activation with the channel for sending notifications of signals received.
                // This occurs when re-installed.
                Ok(Installed { notify }) => Continue(Active { notify, controller }),
                // It's already dormant.  No change.  Recur to keep blocking.  This message while
                // we're in this state, does not occur actually.
                Ok(Uninstalled) => {
                    debug_assert!(false, "doesn't occur with current design");
                    Self::control(Dormant { controller })
                },
                // If the controller channel is ever disconnected, that means to finish the
                // thread.
                Err(RecvError) => Break(()),
            },
        }
    }
}


#[derive(Debug)]
pub(super) enum NewError {
    SemaphoreInitFailed(io::Error),
    ThreadCreateFailed(io::Error),
}

impl Display for NewError {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", match self {
            Self::SemaphoreInitFailed(_) => "semaphore initialization failed",
            Self::ThreadCreateFailed(_) => "thread creation failed at OS level",
        })
    }
}

impl Error for NewError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::SemaphoreInitFailed(e) | Self::ThreadCreateFailed(e) => Some(e),
        }
    }
}
