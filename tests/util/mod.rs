#![allow(dead_code)]

use signals_receipts::SignalNumber;


pub(crate) fn raise(signum: SignalNumber) {
    #![allow(unsafe_code)]
    // SAFETY: The argument is proper.
    let r = unsafe { libc::raise(signum) };
    assert_eq!(r, 0, "will succeed");
}


pub(crate) fn send_signal_to_proc(signum: SignalNumber, pid: libc::pid_t) -> bool {
    #![allow(unsafe_code)]
    // SAFETY: The arguments are proper.
    let r = unsafe { libc::kill(pid, signum) };
    if r == 0 {
        true
    } else {
        let errno = errno::errno().0;
        assert_eq!(errno, libc::ESRCH); // The process no longer exists.
        false
    }
}
