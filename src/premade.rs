pub use receipts::*;
mod receipts;

#[doc(hidden)]
// Not for public use.  This must be `pub` so our macro can refer to it when expanded in other
// crates.
pub mod __internal;


use crate::{AtomicUInt, Semaphore, SignalNumber, SignalReceipt};
use __internal::Sealed;
use core::{ops::ControlFlow,
           pin::Pin,
           sync::atomic::{AtomicBool, Ordering::Relaxed}};
use errno::errno;


/// Functions for using a `SignalsReceipts` type to manage the signal handling and processing as
/// generated by the [`premade`](crate::premade!) macro.
///
/// This trait is sealed to only be implemented automatically by the `premade` macro.  This trait
/// only exists so that macro can provide these functions.
pub trait Premade: Sealed {
    /// The type of the accumulator value that is passed in and out of all delegates
    /// during processing.
    type Continue;
    /// The type of the final value that the processing finishes with.
    type Break;

    /// Get the reference to our flag that indicates if the consuming thread should continue
    /// looping to process more or else should finish.
    fn continue_flag() -> &'static AtomicBool;

    /// Get the reference to our semaphore.
    fn semaphore() -> Pin<&'static Semaphore>;

    /// Do [`install_handler()`](crate::install_handler) for all of the declared signal numbers.
    ///
    /// All (non-exceptional) signals will be masked during when `handler` is called upon delivery
    /// of any one of these signals.
    ///
    /// `SA_RESTART` will be enabled so that interruptible functions shall restart if interrupted
    /// by delivery of any one of these signals.
    ///
    /// # Panics
    /// If installing a handler fails.  Only possible if an invalid signal number was given.
    #[inline]
    fn install_all_handlers() { Self::install_all_handlers_with(true, true); }

    /// Do [`install_handler()`](crate::install_handler) for all of the declared signal numbers.
    ///
    /// The arguments are passed to each `install_handler()`.
    ///
    /// # Panics
    /// If installing a handler fails.  Only possible if an invalid signal number was given.
    fn install_all_handlers_with(mask: bool, restart: bool);

    /// Do [`uninstall_handler()`](crate::uninstall_handler) for all of the declared signal
    /// numbers.
    ///
    /// # Panics
    /// If installing the default for a signal fails.  Only possible if an invalid signal number
    /// was given.
    fn uninstall_all_handlers();

    /// Assign zero to each counter, for all of the declared signal numbers.
    fn reset_all_counters();

    /// Intended to be used as (or within) the start function of a dedicated thread.
    ///
    /// All non-exceptional signals will be masked for the current thread.
    #[must_use]
    #[inline]
    fn consume_loop() -> Self::Break
    where
        Self::Continue: Default,
        Self::Break: Default,
    {
        Self::consume_loop_with(true, Default::default(), Default::default())
    }

    /// Intended to be used as (or within) the start function of a dedicated thread.
    ///
    /// The current signal mask will not be changed and will be left as-is.
    #[must_use]
    #[inline]
    fn consume_loop_no_mask() -> Self::Break
    where
        Self::Continue: Default,
        Self::Break: Default,
    {
        Self::consume_loop_with(false, Default::default(), Default::default())
    }

    /// Intended to be used as (or within) the start function of a dedicated thread.
    ///
    /// Enables more control over the parameters, which are passed to [`crate::consume_loop`].  Is
    /// necessary when `Self`'s associated types don't both `impl`ement `Default`.
    #[must_use]
    fn consume_loop_with(
        do_mask: bool,
        accum: Self::Continue,
        finish: Self::Break,
    ) -> Self::Break;

    /// Finish all processing, by uninstalling all handlers and indicating to the consuming thread
    /// that it should finish.
    ///
    /// # Panics
    /// Same as [`Self::uninstall_all_handlers`].
    #[inline]
    fn finish() {
        // Reset the dispositions and stop counting signal deliveries.
        Self::uninstall_all_handlers();

        // Tell the consuming thread to finish.
        Self::continue_flag().store(false, Relaxed);

        // Ensure the thread wakes to see the false continue-flag now.
        if let Some(sem) = Self::semaphore().try_init(100) {
            // Our change to the flag will be visible, as happens-before, to the thread that
            // wakes.
            let r = sem.post();
            #[allow(clippy::unreachable)]
            if r.is_err() {
                let errno = errno().0;
                if errno == libc::EOVERFLOW {
                    // The maximum allowable value of the semaphore would be exceeded.  We just
                    // live with this, because the other consuming thread will continue to see the
                    // semaphore have a very-high positive value when doing `sem_wait()` and so it
                    // won't block and will continue to process the false continue-flag.
                } else {
                    unreachable!(); // Impossible.
                }
            }
        } else {
            // Our semaphore wasn't already initialized and couldn't be quickly.  This is very
            // unlikely, and there's nothing we can do, but at least we did set the continue-flag.
        }
    }
}


/// A premade pattern of statically declaring which signal numbers need to be processed and how to
/// do so, with a premade function to run as a thread dedicated to consuming their receipts and
/// dispatching the declared processing, with premade defaults for the finer details.
///
/// Expands to the definition of a module that defines a type named `SignalsReceipts` that
/// `impl`ements the [`Premade`] trait that is used to manage the signal handling and processing.
///
/// The name of the module defaults to `signals_receipts_premade` when not given.
///
/// The `Continue` and `Break` types default to `()` when not given.
#[macro_export]
macro_rules! premade {
    {
        $( ( $( $item:item )* ) )?
        $( {callback} => $callback:expr; )?
        $( $signum:ident => $delegate:expr; )+
    } => {
        $crate::premade! {
            $( ( $( $item )* ) )?
            type Continue = ();
            type Break = ();
            $( {callback} => $callback; )?
            $( $signum => $delegate; )+
        }
    };

    {
        $( ( $( $item:item )* ) )?
        type Continue = $cont:ty;
        type Break = $break:ty;
        $( {callback} => $callback:expr; )?
        $( $signum:ident => $delegate:expr; )+
    } => {
        $crate::premade! {
            mod signals_receipts_premade {
                $( ( $( $item )* ) )?
                type Continue = $cont;
                type Break = $break;
                $( {callback} => $callback; )?
                $( $signum => $delegate; )+
            }
        }
    };

    {
        $visib:vis mod $name:ident {
            $( ( $( $item:item )* ) )?
            $( {callback} => $callback:expr; )?
            $( $signum:ident => $delegate:expr; )+
        }
    } => {
        $crate::premade! {
            $visib mod $name {
                $( ( $( $item )* ) )?
                type Continue = ();
                type Break = ();
                $( {callback} => $callback; )?
                $( $signum => $delegate; )+
            }
        }
    };

    {
        $visib:vis mod $name:ident {
            $( ( $( $item:item )* ) )?
            type Continue = $cont:ty;
            type Break = $break:ty;
            $( {callback} => $callback:expr; )?
            $( $signum:ident => $delegate:expr; )+
        }
    } => {
        $visib mod $name {
            use $crate::{consume_count_then_delegate, install_handler, uninstall_handler,
                         reset_counter, __internal::{signals_names, Sealed},
                         Consumer, Premade, Semaphore, SemaphoreRef, SignalReceipt};
            use core::{pin::Pin, sync::atomic::{AtomicBool, AtomicU64}};

            /// The type that [`SignalReceipt`] and [`Premade`] are `impl`emented for.
            ///
            /// This being `pub`lic can also be useful as the `T` with the items of the
            /// `signals_receipts` API that require `T: SignalReceipt<SIGNUM>`.  E.g. with
            /// [`install_handler`] or [`consume_count_then_delegate`].
            pub(crate) struct SignalsReceipts;

            $(
                impl SignalReceipt<{signals_names::$signum}> for SignalsReceipts {
                    type AtomicUInt = AtomicU64;

                    fn counter() -> &'static Self::AtomicUInt {
                        static COUNTER: AtomicU64 = AtomicU64::new(0);
                        &COUNTER
                    }

                    fn semaphore() -> Option<SemaphoreRef<'static>> {
                        <Self as Premade>::semaphore().sem_ref().ok()
                    }
                }
            )+

            impl Sealed for SignalsReceipts {}

            impl Premade for SignalsReceipts {
                type Continue = $cont;
                type Break = $break;

                fn semaphore() -> Pin<&'static Semaphore> {
                    static SEMAPHORE: Semaphore = Semaphore::new();
                    Pin::static_ref(&SEMAPHORE)
                }

                fn install_all_handlers_with(mask: bool, restart: bool) {
                    $( install_handler::<{signals_names::$signum}, Self>(mask, restart); )+
                }

                fn uninstall_all_handlers() {
                    $( uninstall_handler::<{signals_names::$signum}>(); )+
                }

                fn reset_all_counters() {
                    $( reset_counter::<{signals_names::$signum}, Self>(); )+
                }

                fn consume_loop_with(
                    do_mask: bool,
                    accum: Self::Continue,
                    finish: Self::Break
                ) -> Self::Break
                {
                    // This just enables our `$( ... $callback ...)?` to work where `$callback`
                    // actually isn't used in that.
                    #[allow(unused_macros)]
                    macro_rules! repeat_for { ($metavar:tt: $second:expr) => { $second } }

                    let sem = <Self as Premade>::semaphore();
                    let try_init_limit = 200_000_000; // Enough for at least a second.
                    let mut consumers = [ $(
                        &mut repeat_for!($callback: delegates::callback::__FUNC)
                            as &mut Consumer<Self::Break, Self::Continue>,
                    )? $(
                        &mut (|accum| consume_count_then_delegate::<
                              {signals_names::$signum}, Self, _, Self::Break, Self::Continue>(
                                  accum, delegates::$signum::__FUNC))
                            as &mut Consumer<Self::Break, Self::Continue>
                    ),+ ];
                    let continue_flag = <Self as Premade>::continue_flag();

                    $crate::consume_loop(do_mask, sem, try_init_limit, accum, &mut consumers,
                                         continue_flag, finish)
                }

                fn continue_flag() -> &'static AtomicBool {
                    static CONTINUE_FLAG: AtomicBool = AtomicBool::new(true);
                    &CONTINUE_FLAG
                }
            }

            /// Places the `$delegate` expressions in (nearly) clean scopes, so they cannot
            /// (easily) refer to any identifiers in this macro's `mod $name` module.
            #[allow(non_snake_case, unreachable_pub)]
            mod delegates {
                $( $( $item )* )?  // Enables giving imports & items, for the delegates.
                $(
                    pub(super) mod callback {
                        use super::*; // Import any items given above.

                        pub(in super::super) const __FUNC:
                          fn(<super::super::SignalsReceipts as $crate::Premade>::Continue)
                            -> core::ops::ControlFlow<
                                 <super::super::SignalsReceipts as $crate::Premade>::Break,
                                 <super::super::SignalsReceipts as $crate::Premade>::Continue>
                          = $callback;
                    }
                )?
                $(
                    pub(super) mod $signum {
                        use super::*; // Import any items given above.

                        pub(in super::super) const __FUNC:
                          fn(&mut $crate::Receipt<u64,
                                    <super::super::SignalsReceipts as $crate::Premade>::Break,
                                    <super::super::SignalsReceipts as $crate::Premade>::Continue>)
                          = $delegate;
                    }
                )+
            }
        }
    };
}


/// The common pattern of taking the current count, of how many times the signal specified by
/// `SIGNUM` has been delivered, and delegating to a given function or closure to process, the
/// [`Receipt`] representation of, that however desired.
///
/// Intended to be called from within a [`Consumer`](crate::Consumer) given to
/// [`consume_loop()`](crate::consume_loop) (or the like).
#[must_use]
#[inline]
pub fn consume_count_then_delegate<const SIGNUM: SignalNumber, T, F, B, C>(
    accum: C,
    mut delegate: F,
) -> ControlFlow<B, C>
where
    T: SignalReceipt<SIGNUM>,
    F: FnMut(&mut Receipt<<<T as SignalReceipt<SIGNUM>>::AtomicUInt as AtomicUInt>::UInt, B, C>),
{
    let cur_count = <T as SignalReceipt<SIGNUM>>::take_count();
    let flow = ControlFlow::Continue(accum);
    if cur_count == 0.into() {
        // Do not call the delegate, when the count is zero.
        flow
    } else {
        // Passing-in this kind of argument enables a delegate to be simpler in which aspects it
        // wants to deal with or not.
        let mut receipt = Receipt { sig_num: SIGNUM, cur_count, flow };
        delegate(&mut receipt);
        receipt.flow // The delegate can choose whether or not to change this.
    }
}
