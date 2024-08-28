// MAYBE: This could become a separate library published on Crates.io in the future, maybe named
// `signals_utils`.

#![allow(unsafe_code)]

pub(crate) use sigaction::SigAction;


macro_rules! except_signals {
    () => {
        "\n\nExcept for: `SIGFPE`, `SIGILL`, `SIGSEGV`, `SIGBUS` (those that must not be blocked \
         if a \"computational exception\" occurs); and `SIGABRT`, `SIGSYS`, `SIGTRAP`, `SIGIOT`, \
         `SIGEMT` (those that should always be delivered); and `SIGKILL`, `SIGSTOP` (those that \
         cannot be blocked anyway)."
    };
}


#[allow(unreachable_pub)] // These full-`pub`s are in case this becomes a separate library.
mod sigaction {
    use super::{sigset_all_usual, sigset_empty};
    use crate::SignalNumber;
    use core::{ffi::{c_int, c_void},
               mem::MaybeUninit,
               ptr::{self, addr_of, addr_of_mut}};

    // The `libc` crate is inconsistent across different OSs and LibCs about whether or not the
    // `.sa_handler` field is provided.  When it's not, the `.sa_sigaction` field has to be used
    // instead as the `.sa_handler` field (which is alright because they're in a `union` at the
    // same offset, in this case).
    #[rustfmt::skip]
    cfg_if::cfg_if! {
        if #[cfg(any(
            all(target_os = "linux", any(target_env = "gnu", target_env = "musl")),
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "illumos",
            target_os = "macos",
        ))] {
            macro_rules! sa_handler_cfg { ($obj:expr) => { ($obj).sa_sigaction }; }
        }
        // Unsupported
        else {
            macro_rules! sa_handler_cfg { ($obj:expr) => {
                core::compile_error!("Platform not supported yet. You may add support.")
            }; }
        }
    }

    /// Pointer to a signal-catching function of the non-`SA_SIGINFO` type.
    pub type Handler = extern "C" fn(signo: SignalNumber);

    // MAYBE: These could be exposed in the future.
    /// Pointer to a signal-catching function of the `SA_SIGINFO` type.
    type HandlerWithInfo =
        extern "C" fn(signo: SignalNumber, info: *mut SigInfo, context: *mut c_void);
    // This exists to avoid exposing in the API our use of the `libc` crate.
    type SigInfo = libc::siginfo_t;

    /// A builder of a C `struct sigaction` that can only be used safely.
    #[must_use]
    #[derive(Debug)]
    #[allow(missing_copy_implementations)]
    pub struct SigAction(MaybeUninit<libc::sigaction>);

    impl Default for SigAction {
        /// Set the `.sa_handler` field to `SIG_DFL`.
        #[inline]
        fn default() -> Self {
            // SAFETY: The argument is one of the allowed values.
            unsafe { Self::sa_handler(libc::SIG_DFL) }
        }
    }

    impl SigAction {
        fn new() -> Self {
            let mut it = Self(MaybeUninit::<libc::sigaction>::zeroed());

            // Initialize these two fields immediately, in case nothing else does.

            let sa_mask = it.sa_mask_mut_ptr();
            // SAFETY: The argument is valid, aligned, and unaliased. It's allowed to be
            // uninitialized.
            unsafe {
                sigset_empty(sa_mask); // In case zeroes isn't "empty".
            }

            let sa_flags = it.sa_flags_mut_ptr();
            // SAFETY: `sa_flags` is valid, aligned, unaliased, and uninitialized.
            unsafe {
                sa_flags.write(0); // Ensure that Rust considers this as written to.
            }

            it
        }

        #[cfg_attr(not(debug_assertions), allow(dead_code))]
        fn sa_handler_ptr(&self) -> *const libc::sighandler_t {
            let act = self.0.as_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of!(sa_handler_cfg!(*act)) }
        }

        fn sa_handler_mut_ptr(&mut self) -> *mut libc::sighandler_t {
            let act = self.0.as_mut_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of_mut!(sa_handler_cfg!(*act)) }
        }

        #[allow(dead_code)]
        fn sa_mask_ptr(&self) -> *const libc::sigset_t {
            let act = self.0.as_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of!((*act).sa_mask) }
        }

        fn sa_mask_mut_ptr(&mut self) -> *mut libc::sigset_t {
            let act = self.0.as_mut_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of_mut!((*act).sa_mask) }
        }

        #[cfg_attr(not(debug_assertions), allow(dead_code))]
        fn sa_flags_ptr(&self) -> *const c_int {
            let act = self.0.as_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of!((*act).sa_flags) }
        }

        fn sa_flags_mut_ptr(&mut self) -> *mut c_int {
            let act = self.0.as_mut_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of_mut!((*act).sa_flags) }
        }

        #[cfg_attr(not(debug_assertions), allow(dead_code))]
        fn sa_sigaction_ptr(&self) -> *const libc::sighandler_t {
            let act = self.0.as_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of!((*act).sa_sigaction) }
        }

        fn sa_sigaction_mut_ptr(&mut self) -> *mut libc::sighandler_t {
            let act = self.0.as_mut_ptr();
            // SAFETY: The pointers to the field and the struct are in-bounds.
            unsafe { addr_of_mut!((*act).sa_sigaction) }
        }

        /// # Safety:
        /// `val` must be the address of a function of type [`Handler`], or must be `SIG_DFL` or
        /// `SIG_IGN`.
        unsafe fn sa_handler(val: libc::uintptr_t) -> Self {
            let mut it = Self::new();
            let sa_handler = it.sa_handler_mut_ptr();
            // SAFETY: `sa_handler` is valid, aligned, unaliased, and uninitialized.
            unsafe {
                sa_handler.write(val);
            }
            it
        }

        /// # Safety:
        /// `val` must be the address of a function of type [`HandlerWithInfo`].
        unsafe fn sa_sigaction(val: libc::uintptr_t) -> Self {
            let mut it = Self::new();
            let sa_flags = it.sa_flags_mut_ptr();
            // SAFETY: `sa_flags` is valid, aligned, unaliased, and initialized.
            unsafe {
                *sa_flags |= libc::SA_SIGINFO;
            }
            let sa_sigaction = it.sa_sigaction_mut_ptr();
            // SAFETY: `sa_sigaction` is valid, aligned, unaliased, and uninitialized.
            unsafe {
                sa_sigaction.write(val);
            }
            it
        }

        /// Set the `.sa_handler` field to `handler`.
        #[inline]
        pub fn handler(handler: Handler) -> Self {
            // SAFETY: The argument is the address of the function of type `Handler`.
            unsafe {
                #[allow(clippy::fn_to_numeric_cast_any, clippy::as_conversions)]
                Self::sa_handler(handler as usize)
            }
        }

        // MAYBE: This could be exposed in the future.
        #[allow(dead_code)]
        /// Set the `.sa_sigaction` field to `handler`.
        fn handler_with_info(handler: HandlerWithInfo) -> Self {
            // SAFETY: The argument is the address of the function of type `HandlerWithInfo`.
            unsafe {
                #[allow(clippy::fn_to_numeric_cast_any, clippy::as_conversions)]
                Self::sa_sigaction(handler as usize)
            }
        }

        /// Set the `.sa_handler` field to `SIG_IGN`.
        #[allow(dead_code)]
        #[inline]
        pub fn ignore() -> Self {
            // SAFETY: The argument is one of the allowed values.
            unsafe { Self::sa_handler(libc::SIG_IGN) }
        }

        /// Mask (almost) all signals during execution of the signal handler.
        #[doc = except_signals!()]
        #[inline]
        pub fn mask_all(mut self) -> Self {
            let sa_mask = self.sa_mask_mut_ptr();
            // SAFETY: The argument is valid, aligned, and unaliased.
            unsafe {
                sigset_all_usual(sa_mask);
            };
            self
        }

        /// Set the `.sa_flags` field to include `SA_RESTART`.
        #[inline]
        pub fn restart_intr(mut self) -> Self {
            let sa_flags = self.sa_flags_mut_ptr();
            // SAFETY: `sa_flags` is valid, aligned, unaliased, and initialized.
            unsafe {
                *sa_flags |= libc::SA_RESTART;
            }
            self
        }

        /// Like [`sigaction`](
        /// https://pubs.opengroup.org/onlinepubs/9799919799/functions/sigaction.html).
        ///
        /// # Errors
        /// If `sigaction()` does.  `errno` is set to indicate the error.
        ///
        /// # Safety
        /// The creator of `self` must ensure that its handler is async-signal-safe.
        #[allow(clippy::result_unit_err)]
        #[inline]
        pub unsafe fn install(self, signum: SignalNumber) -> Result<(), ()> {
            #[cfg(debug_assertions)]
            {
                let sa_flags = self.sa_flags_ptr();
                // SAFETY: `sa_flags` is valid, aligned, initialized, and `Copy`.
                let flags = unsafe { *sa_flags };
                let is_siginfo = flags & libc::SA_SIGINFO != 0;

                if is_siginfo {
                    let sa_sigaction = self.sa_sigaction_ptr();
                    debug_assert!(!sa_sigaction.is_null(), "must not be null");
                } else {
                    let sa_handler = self.sa_handler_ptr();
                    debug_assert!(!sa_handler.is_null(), "must not be null");
                }
            }
            // SAFETY: Each of the constructors of `Self` sufficiently initializes by itself and
            // further builder methods ensure the initialization remains proper.
            let act = unsafe { self.0.assume_init() };
            // SAFETY: The arguments are proper, because `act` was initialized.
            let r = unsafe { libc::sigaction(signum, &act, ptr::null_mut()) };
            if r == 0 { Ok(()) } else { Err(()) }
            // MAYBE: In the future, the previously-associated action could be returned as a new
            // `Self` which would already be initialized (via the 3rd arg of the `sigaction()`
            // above).
        }
    }
}


/// Initializes a `sigset_t` to have almost all signals set.
#[doc = except_signals!()]
///
/// # Safety:
/// The argument must be valid, aligned, and unaliased. It's allowed to be uninitialized.
#[allow(clippy::unnecessary_safety_comment)] // Suppress Clippy bug.
unsafe fn sigset_all_usual(set: *mut libc::sigset_t) {
    // SAFETY: The caller must uphold the safety.
    let r1 = unsafe { libc::sigfillset(set) };
    debug_assert_eq!(0, r1, "`sigfillset()` never errors");

    for must_not in [
        // If a "computational exception" occurs in a thread, one of these will be generated and
        // delivered to the thread.  It'd be undefined behavior if the thread were to continue
        // executing after that.  These signals must not be masked ("blocked") in any thread, so
        // they'll still be delivered (and terminate the process, usually) if a "computational
        // exception" occurs (to prevent continuing to execute undefined behavior).
        libc::SIGFPE,
        libc::SIGILL,
        libc::SIGSEGV,
        libc::SIGBUS,
        // These should always be delivered if generated.
        libc::SIGABRT,
        libc::SIGSYS,
        libc::SIGTRAP,
        libc::SIGIOT,
        #[cfg(any(
            // SIGEMT is present in these OSs, for all CPU architectures it seems.
            target_os = "freebsd", target_os = "netbsd", target_os = "illumos",
            target_os = "macos",
            // SIGEMT is not present for Linux x86.
            not(all(target_os = "linux", any(target_arch = "x86", target_arch = "x86_64")))
            // For all other OSs or architectures, which this crate currently doesn't have support
            // for yet, assume SIGEMT is present, so if it's not then this will error and this
            // `cfg` can be adjusted for that platform's lack of it.
        ))]
        libc::SIGEMT,
        // These cannot be "blocked" anyway, and attempting to do so would be ignored and they'd
        // still be delivered anyway.
        libc::SIGKILL,
        libc::SIGSTOP,
    ] {
        // SAFETY: The arguments are proper, because `set` was initialized.
        let r2 = unsafe { libc::sigdelset(set, must_not) };
        debug_assert_eq!(0, r2, "will succeed, because all are supported signal numbers");
    }
}

/// Initializes a `sigset_t` to have no signals set.
///
/// # Safety:
/// The argument must be valid, aligned, and unaliased. It's allowed to be uninitialized.
#[allow(clippy::unnecessary_safety_comment)] // Suppress Clippy bug.
unsafe fn sigset_empty(set: *mut libc::sigset_t) {
    // SAFETY: The caller must uphold the safety.
    let r = unsafe { libc::sigemptyset(set) };
    debug_assert_eq!(0, r, "`sigemptyset()` never errors");
}


/// Changes the calling thread's signal mask to "block" (prevent from being delivered) almost all
/// signals.
#[doc = except_signals!()]
///
/// Note that other threads can still receive any signals depending on their masks, which might
/// depend on whether or not they inherit the calling thread's mask.
#[inline]
pub fn mask_all_signals_of_current_thread() {
    use core::{mem::MaybeUninit, ptr};

    let set = {
        let mut set = MaybeUninit::<libc::sigset_t>::zeroed();
        // SAFETY: The argument is valid, aligned, and unaliased. It's allowed to be
        // uninitialized.
        unsafe {
            sigset_all_usual(set.as_mut_ptr());
        }
        // SAFETY: We just initialized it.
        unsafe { set.assume_init() }
    };

    // SAFETY: The arguments are proper, because `set` was initialized.
    let r = unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &set, ptr::null_mut()) };
    debug_assert_eq!(0, r, "will succeed");
}


/// An async-signal-safe "panic" that can be used from within a signal handler.
#[inline]
pub(crate) fn abort(msg: &[u8]) -> ! {
    fn ewrite(msg: &[u8]) {
        use core::{ffi::c_void, hint};
        const LIMIT: u16 = 10;
        let msg_buf: *const [u8] = msg;
        let msg_buf: *const c_void = msg_buf.cast();
        let mut remaining = msg.len();
        for _ in 0 .. LIMIT {
            if remaining >= 1 {
                // SAFETY: The arguments are proper, because `msg` is a safe type and `remaining`
                // is correct.
                let r = unsafe { libc::write(libc::STDERR_FILENO, msg_buf, remaining) };
                match usize::try_from(r) {
                    Ok(written) => remaining = remaining.saturating_sub(written),
                    Err(_) => break, // `r == -1`, failure to write.
                }
            } else {
                break;
            }
            hint::spin_loop(); // Might as well slow it down a tiny bit.
        }
    }

    ewrite(b"Internal Abort: ");
    ewrite(msg);

    // SAFETY: The call is proper.
    unsafe {
        libc::abort();
    }
}
