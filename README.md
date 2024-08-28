# `signals_receipts`

An approach to handling POSIX signals that is simple, lightweight, async-signal-safe, and
portable.

Each signal number of interest is associated with: an atomic counter of how many times that signal
has been delivered since it was last checked, and your custom processing for that signal.  A
semaphore is posted when any signal of interest is delivered, to wake a consumer thread that
checks all the counters and delegates to your custom processing which is run in a normal context
(not in the interrupt context of a signal handler, which would be extremely limited by the
requirement to only do async-signal-safe things).

# Example

```rust no_run
// This defines the `signals_receipts_premade` module.
signals_receipts::premade! {
    SIGINT => |receipt| println!("Interrupted {} times since last.", receipt.cur_count);
    SIGTERM => |control| control.break_loop();
}

fn main() {
    use crate::signals_receipts_premade::SignalsReceipts;
    use signals_receipts::Premade as _;

    SignalsReceipts::install_all_handlers();
    let consumer = std::thread::spawn(SignalsReceipts::consume_loop);
    consumer.join();
    println!("Terminated.");
}
```

# Motivation

This crate is intended for only POSIX OSs, when only counters and only a single delegate per
signal number are sufficient.  Not for when the extra info provided via `SA_SIGINFO` is needed.
Not for when multiple delegates per signal number is needed (though, you could make something like
that with this crate).  Not for supporting Windows.  Having any of those abilities would be too
involved for this crate.

Using [POSIX Semaphores](https://crates.io/crates/sem_safe), which this crate does, is a little
simpler and cleaner than the classic "self-pipe trick" (which the `signal_hook` crate uses for its
iterator), for waking a consumer thread from within an extremely-limited signal handler.  (The
"self-pipe trick" is: `write()` to a pipe is done from a signal handler, and blocking `read()`
from the other end of the pipe is done from the consumer thread.  That is somewhat messier (due to
needing to: setup the pipes, close-on-exec, non-blocking writes, and make the ends accessible to
the threads).)

The other classic approach of using `sigwait` (or one of its variants) from a consumer thread, to
avoid async-signal handlers altogether, is sometimes too undesirable because of its requirement to
mask all signals in all threads which interferes with the signal masks of all subprocesses
(i.e. child processes inherit the parent's all-masked signal mask across `exec()` which can break
their programs, unless carefully reset for each).  This crate is suitable for when that approach
is not done, i.e. for when async-signal handlers are used.

This crate exposes as public some of its mechanisms, in case they're useful for you to customize
your use to be somewhat different than this crate's `premade` macro's choices.

# Alternative

The [`signal_hook`](https://crates.io/crates/signal-hook) crate provides an impressive degree of
abilities, for being so limited by async-signal-safety.  Its iterator over incoming signals is
simple to initialize and use across threads, and using only that is sometimes sufficient.  It can
provide the extra info of `SA_SIGINFO`.  A reason to not use `signal_hook` is when having its full
suite of abilities, but mostly unused, would definitely be overkill.  If you're not sure it would
be overkill, you might want to choose `signal_hook` instead.  But another reason to use
`signals_receipts` is that it can fully uninstall the signal handlers, whereas `signal_hook` can't
do that (it can only emulate that mostly).

# Portability

This crate was confirmed to build and pass its tests and examples on (x86_64 only so far):

- BSD
  - FreeBSD 14.0
  - NetBSD 9.1
  - OpenBSD 7.5
- Linux
  - Adélie 1.0 (uses musl)
  - Alpine 3.18 (uses musl)
  - Chimera (uses musl)
  - Debian 12
  - NixOS 24.05
  - openSUSE 15.5
  - RHEL (Rocky) 9.4
  - Ubuntu 23.10
- Mac
  - 10.13 High Sierra
  - 12 Monterey
- Solaris
  - OpenIndiana 2024.04

All glibc- or musl-based Linux OSs, and all macOS and Mac OS X versions, should already work.  It
might already work on further POSIX OSs.  If not, adding support for other POSIX OSs should be
easy but might require making tweaks to this crate's conditional compilation.
