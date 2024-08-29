use errno::errno;


/// Only intended to be called after `SemaphoreRef::post()` to check its result.
///
/// It's impossible that `impossible` will ever be called, but it's given just to have an
/// "unreachable" or "abort" for that branch of the conditional.
pub(crate) fn assert_errno_is_overflow(impossible: fn() -> !) {
    let errno = errno().0;
    if errno == libc::EOVERFLOW {
        // The maximum allowable value of the semaphore would be exceeded.  We just live with
        // this, because the other consuming thread will continue to see the semaphore have a
        // very-high positive value when doing `sem_wait()` and so it won't block and will
        // continue to process.
    } else {
        impossible(); // Impossible - `sem_safe` ensures the semaphores are valid.
    }
}
