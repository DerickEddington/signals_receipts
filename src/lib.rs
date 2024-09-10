#![doc = include_str!("../README.md")]
#![no_std]
#[cfg(not(target_family = "unix"))]
core::compile_error!("Only supported on POSIX.");


pub use atomics::*;
mod atomics;

cfg_if::cfg_if! { if #[cfg(feature = "premade")] {
    pub use premade::*;
    mod premade;
} }

/// Helpers that are sometimes useful when using this crate.
pub mod util;

mod help;


use core::{ffi::c_int,
           ops::ControlFlow,
           pin::Pin,
           sync::atomic::{AtomicBool, Ordering::Relaxed}};
use errno::{errno, set_errno};
use help::assert_errno_is_overflow;
// These are re-exported because they're exposed in our public API.
#[doc(no_inline)]
pub use sem_safe::{non_named::Semaphore as SemaphoreMethods, plaster::non_named::Semaphore,
                   SemaphoreRef};
use util::{abort, mask_all_signals_of_current_thread, unmask_all_signals_of_current_thread,
           SigAction};


/// The type of a signal number as defined by C (C17 7.14).
pub type SignalNumber = c_int;

/// The ability to handle receipt of a particular signal.
///
/// All implementations of these methods must be async-signal-safe, because they're called from
/// within an async-signal handler in an interrupt context.
pub trait SignalReceipt<const SIGNUM: SignalNumber> {
    /// An unsigned integer type with atomic operations as needed by this crate.
    type AtomicUInt: AtomicUInt;

    /// Atomically replace the value referred to by [`Self::counter()`] with zero and return its
    /// previous value.
    #[must_use]
    #[inline]
    fn take_count() -> <Self::AtomicUInt as AtomicUInt>::UInt {
        Self::counter().swap(0.into(), Relaxed)
    }

    /// Get the reference to the counter that counts how many times the signal specified by
    /// `SIGNUM` has been delivered.
    ///
    /// The lifetime must be `'static` because a signal handler, that accesses a counter, can live
    /// for the rest of the duration of a program once installed.
    #[must_use]
    fn counter() -> &'static Self::AtomicUInt;

    /// Get the reference to the semaphore that wakes the "consuming" thread when a signal of
    /// interest has been delivered.  Return `None` if the semaphore is not yet initialized.
    ///
    /// The "consuming" thread should wait by blocking on calling [`SemaphoreRef::wait`].
    ///
    /// The lifetime must be `'static` because a signal handler, that accesses a semaphore, can
    /// live for the rest of the duration of a program once installed.
    #[must_use]
    fn semaphore() -> Option<SemaphoreRef<'static>>;
}


/// A signal handler that increments a receipt counter and posts a semaphore.
///
/// Everything done in this is async-signal-safe.
#[allow(clippy::missing_inline_in_public_items)]
pub extern "C" fn handler<const SIGNUM: SignalNumber, T: SignalReceipt<SIGNUM>>(
    _signo: SignalNumber,
) {
    #[cfg(debug_assertions)]
    #[allow(clippy::used_underscore_binding)]
    if _signo != SIGNUM {
        abort(b"must only be installed for the corresponding `const SIGNUM`.");
    }

    // A signal handler must restore `errno` if it might alter it.
    let prev_errno = errno();

    T::counter().saturating_incr();

    if let Some(sem) = T::semaphore() {
        // Our change to the counter will be visible, as happens-before, to the thread that wakes.
        let r = sem.post();
        if r.is_err() {
            assert_errno_is_overflow(|| {
                // Impossible - `sem_safe` ensures the semaphores are valid.  Unreachable.  But
                // `unreachable!()` can't be used, because panicking is not async-signal-safe.
                abort(b"`sem_post()` errored!");
            });
            set_errno(prev_errno);
        }
    } else {
        // The semaphore isn't initialized or ready yet.  We still incremented the receipt
        // counter, which a consuming thread can still detect when it's ready.
    }
}

/// Install [`handler`] for the given `SIGNUM`, using the given `SignalReceipt<SIGNUM>`
/// implementation.
///
/// If `mask`, all (non-exceptional) signals will be masked during when `handler` is called upon
/// delivery of this signal.
///
/// If `restart`, `SA_RESTART` will be enabled so that interruptible functions shall restart if
/// interrupted by delivery of this signal.
///
/// # Panics
/// If installing the handler fails.  Only possible if an invalid signal number was given.
#[inline]
pub fn install_handler<const SIGNUM: SignalNumber, T: SignalReceipt<SIGNUM>>(
    mask: bool,
    restart: bool,
) {
    #![allow(unsafe_code, clippy::expect_used)]

    let mut action = SigAction::handler(handler::<SIGNUM, T>);
    if mask {
        action = action.mask_all();
    }
    if restart {
        action = action.restart_intr();
    }
    // SAFETY: `handler` is async-signal-safe.
    let r = unsafe { action.install(SIGNUM) };
    r.expect("signal number should be valid");
}

/// Uninstall whatever handler might be installed for the given `SIGNUM`, by resetting its
/// disposition to its default.
///
/// # Panics
/// If installing the default fails.  Only possible if an invalid signal number was given.
#[inline]
pub fn uninstall_handler<const SIGNUM: SignalNumber>() {
    #![allow(unsafe_code, clippy::expect_used)]

    let action = SigAction::default();
    // SAFETY: `SIG_DFL` handling is async-signal-safe, because no user function is called.
    let r = unsafe { action.install(SIGNUM) };
    r.expect("signal number should be valid");
}

/// Assign zero to the counter of the given `SIGNUM`, using the given `SignalReceipt<SIGNUM>`
/// implementation.
#[inline]
pub fn reset_counter<const SIGNUM: SignalNumber, T: SignalReceipt<SIGNUM>>() {
    let _count = <T as SignalReceipt<SIGNUM>>::take_count();
}


/// A function or closure to call from [`consume_loop()`] (or the like) to process receipt of one
/// (or more) signal(s).
///
/// The variant of [`ControlFlow`] that is returned controls whether the consuming loop continues
/// to process subsequent receipts or breaks to finish immediately.
pub type Consumer<B = (), C = ()> = dyn FnMut(C) -> ControlFlow<B, C>;

/// The common pattern of a thread that is woken to process signals that were received.
///
/// Intended to be used as (or within) the start function of a dedicated thread.
///
/// This function, and so the given `consumers` functions also, are executed in a normal context,
/// and so they can use things like normal, i.e. not be limited by async-signal-safety.
///
/// If `do_mask` is `true`, all non-exceptional signals will be masked to be blocked for the
/// current thread.  If it's `false`, all signals will be unmasked to be unblocked, in which case
/// the given `consumers` functions must remain correct when interrupted by signals.
///
/// If `try_init_limit` is positive, initializing `sem` will be retried up to that many times,
/// which can be useful if other threads might race to initialize it.  Another thread that is
/// currently executing the initialization will take a short time, in which case it can be useful
/// to retry until that completes.
///
/// # Panics
/// - If semaphore operations fail due to the given `sem`, or the system's limits on semaphores,
///   being in an unusual state.  Won't happen when used as intended.
/// - If one of the given `consumers` does.
#[inline]
pub fn consume_loop<B, C>(
    do_mask: bool,
    sem: Pin<&Semaphore>,
    try_init_limit: u64,
    mut state: C,
    consumers: &mut [&mut Consumer<B, C>],
    continue_flag: &AtomicBool,
    finish: B,
) -> B {
    if do_mask {
        // If signal(s) are delivered to this thread before we mask to prevent that, our handler
        // will be called as usual, and everything will still work because we check the counters
        // before our blocking wait.

        // Prevent this thread from handling the application's signals, so that it's more
        // efficient at making progress on processing their receipts.  This seems to be helpful
        // when very many signals are incoming.  Also, this prevents `EINTR` of our `sem_wait()`
        // (which can be handled (see below) such that our loop would still work, but that isn't
        // quite as simple).
        mask_all_signals_of_current_thread();
    } else {
        // Ensure this thread can and probably will have the application's signals delivered to it
        // and handled on it.  This should work fine as long as the consumer functions can weather
        // being interrupted.  This could be a reasonable choice if an application wants to mask
        // signals for all other threads and have only this thread be dedicated to doing both
        // their async-delivery handling and the receipt processing.  This forces this thread's
        // signal mask to have all signals unblocked, which is helpful when the application has
        // already blocked them for all other threads and is starting a new thread for this
        // function, e.g. when re-installing this receipts processing after it'd already been
        // uninstalled and finished before.
        unmask_all_signals_of_current_thread();
    }

    // Initialize the semaphore if it's not already, retrying the given amount of times.  This
    // supports various use cases where the semaphore might already be initialized or where other
    // threads might race to do the initialization.
    #[allow(clippy::expect_used)]
    let sem = sem.try_init(try_init_limit).expect("semaphore initialization must succeed");

    let is_discontinue = || !continue_flag.load(Relaxed);

    'outer: loop {
        // Check here also, in case `consumers` is empty.
        if is_discontinue() {
            break finish;
        }

        for consume in &mut *consumers {
            match consume(state) {
                ControlFlow::Continue(val) => state = val,
                ControlFlow::Break(val) => break 'outer val,
            }
            // Check again after each, to notice ASAP, to not call any more once it's toggled.
            if is_discontinue() {
                break 'outer finish;
            }
        }

        // At the end of the loop, wait, in case any signals were received before the semaphore
        // was initialized.  Changes to the counters or to the continue-flag, that happen-before
        // the semaphore is posted to wake us, will be visible to us next.
        let r = sem.wait();
        if do_mask {
            #[allow(clippy::expect_used)]
            r.expect("`sem_wait()` will succeed");
        } else if r.is_err() {
            let errno = errno().0;
            assert_eq!(errno, libc::EINTR, "`sem_wait()` will only fail by `EINTR`");
        } else {
            // Succeeded and `!do_mask`.
        }
    }
}
