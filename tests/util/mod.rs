use signals_receipts::SignalNumber;

pub(crate) fn raise(signum: SignalNumber) {
    #![allow(unsafe_code)]
    // SAFETY: The argument is proper.
    let r = unsafe { libc::raise(signum) };
    assert_eq!(r, 0, "will succeed");
}
