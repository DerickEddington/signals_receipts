# `signals_receipts`

An approach to handling POSIX signals that is simple, lightweight, async-signal-safe, and
portable.

Each signal number of interest is associated with: an atomic counter of how many times that signal
has been delivered since it was last checked, and your custom processing for that signal.  A
semaphore is posted when any signal of interest is delivered, to wake a consumer thread that
checks all the counters and delegates to your custom processing which is run in a normal context
(not in the interrupt context of a signal handler, which would be extremely limited by the
requirement to only do async-signal-safe things).

# Examples

<details>
<summary>Simple:</summary>

```rust no_run
// This defines the `signals_receipts_premade` module.
signals_receipts::premade! {
    SIGINT => |receipt| println!("Interrupted {} times since last.", receipt.cur_count);
    SIGTERM => |control| control.break_loop();
}

fn main() {
    use crate::signals_receipts_premade::SignalsReceipts;
    use signals_receipts::Premade as _;
    use std::thread::spawn;

    SignalsReceipts::install_all_handlers();
    let consumer = spawn(SignalsReceipts::consume_loop);
    consumer.join();
    println!("Terminated.");
}
```
</details>

<details>
<summary>Higher-level, over channels, managed facility, requires <code>std</code>:</summary>

```rust no_run
// This defines the `channel_notify_facility_premade` module.
signals_receipts::channel_notify_facility! { SIGINT, SIGQUIT, SIGTERM }

fn main() {
    use crate::channel_notify_facility_premade::SignalsChannel;
    use signals_receipts::{channel_notify_facility::{
                               SignalsChannel as _, Receiver},
                           SignalNumber};

    #[derive(Debug)]
    enum MySignalRepr {
        Interrupt,
        Quit
    }
    impl TryFrom<SignalNumber> for MySignalRepr {
        type Error = &'static str;
        fn try_from(value: SignalNumber) -> Result<Self, Self::Error> {
            match value {
                libc::SIGINT  => Ok(Self::Interrupt),
                libc::SIGQUIT => Ok(Self::Quit),
                _ => Err("unrecognized signal")
            }
        }
    }

    // The capacity of the signals-notifications channel.
    let bound = 10;
    // This installs the signal handlers and creates the consumer thread.
    // Delivered signals are converted to `MySignalRepr`.
    let receiver: Receiver<MySignalRepr, _> =
        SignalsChannel::install(Some(bound)).unwrap();

    for sig in receiver.as_ref() {
        match sig {
            MySignalRepr::Interrupt => println!("Interrupted."),
            MySignalRepr::Quit => { println!("Quitted."); break; },
            // If SIGTERM is delivered while having this install, it's not sent
            // on this channel, because converting it to `MySignalRepr` fails.
        }
    }

    // This uninstalls the signal handlers but keeps the consumer thread
    // as dormant in case re-installing is done later.
    SignalsChannel::uninstall(receiver).unwrap();

    // This re-installs the signal handlers and reuses the same consumer thread.
    // Delivered signals are not converted with this choice of type.
    let receiver: Receiver<SignalNumber, _> =
        SignalsChannel::install(None).unwrap();

    // (Just a way to wait until termination signal.)  SIGTERM is now sent on
    // this different channel, because there's no conversion failure.
    receiver.as_ref().iter().any(|sig_num| libc::SIGTERM == sig_num);

    // This uninstalls and terminates the consumer thread.
    // (Re-installing could still be done again.)
    SignalsChannel::finish(receiver).unwrap();
}
```
</details>

# Motivation

Using [POSIX Semaphores](https://crates.io/crates/sem_safe) is a little simpler and cleaner than
the classic "self-pipe trick", because semaphores avoid having pipes and are a more natural fit
for just waking a consumer thread from within a signal handler.  Using counters enables
immediately incrementing them even when the rest of the processing isn't quite ready yet but can
process them later.  This crate's uninstalling of its signal handling fully uninstalls the signal
handlers at the OS-process level.

These basic abilities of this crate are `no_std`.  This crate exposes as public some of its
mechanisms, in case they're useful for you to customize your use to be somewhat different than
this crate's premade macros' choices.  It's possible to not have a consumer thread and to instead
check the counters and/or semaphore manually wherever and whenever you want.

The other classic approach of using `sigwait` (or one of its variants) from a consumer thread, to
avoid async-signal handlers altogether, is sometimes too undesirable because of its requirement to
mask all signals in all threads which interferes with the signal masks of all subprocesses
(i.e. child processes inherit the parent's all-masked signal mask across `exec()` which can break
their programs, unless carefully reset for each).  This crate is suitable for when that approach
is not done, i.e. for when async-signal handlers are used.

This crate is intended for only POSIX OSs, when only counters and only a single delegate per
signal number are sufficient.  Not for when the extra info provided via `SA_SIGINFO` is needed.
Not for when multiple delegates per signal number is needed (though, you could make something like
that with this crate).  Not for supporting Windows.  Having any of those abilities would be too
involved for this crate.

# Crate Features

- **premade** (on by default) - Enables the premade pattern of statically declaring which signal
  numbers need to be processed and how to do so, with a premade function to run as a thread
  dedicated to consuming their receipts and dispatching the declared processing, with premade
  defaults for the finer details.

- **channel_notify_facility** - Enables the premade facility that sends over channels
  notifications of signals and that manages the installing, uninstalling, and internal consumer
  thread.  Requires the `std` library.

# Alternative

<details>
<summary>
The <a href="https://crates.io/crates/signal-hook"><code>signal_hook</code></a> crate:
</summary>

It provides an impressive degree of abilities, for being so limited by async-signal-safety.  Its
iterator over incoming signals is simple to initialize and use across threads, and using only that
is sometimes sufficient.  It can provide the extra info of <code>SA_SIGINFO</code>.  A reason to
not use <code>signal_hook</code> is when having its full suite of abilities, but mostly unused,
would definitely be overkill.  If you're not sure it would be overkill, you might want to choose
<code>signal_hook</code> instead.  But other reasons to use <code>signals_receipts</code> are that
it's <code>no_std</code> and that it can fully uninstall the signal handlers, whereas
<code>signal_hook</code> isn't (it requires <code>std</code>) and can't (it can only emulate
uninstalling).
</details>

# Portability

<details>
<summary>
This crate was confirmed to build and pass its tests and examples on (x86_64 only so far):
</summary>

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
</details>
