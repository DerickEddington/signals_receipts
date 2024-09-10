#![cfg(test)] // Suppress `clippy::tests_outside_test_module`.
#![allow(
    clippy::assertions_on_result_states,
    clippy::shadow_unrelated,
    clippy::unreachable,
    clippy::unwrap_used,
    unused_crate_dependencies // Ignore the lib crate's deps that are supplied here also.
)]

use channel_notify_facility_premade::SignalsChannel;
use libc::{SIGURG, SIGUSR1, SIGUSR2};
use signals_receipts::{channel_notify_facility::{FinishError, InstallError, Receiver,
                                                 SendError, Sender, SignalsChannel as _,
                                                 UninstallError},
                       SignalNumber};
use std::{sync::mpsc::{self, TryRecvError},
          thread};

#[path = "help/util.rs"]
mod util;
use util::spawn_raise;


signals_receipts::channel_notify_facility! { SIGUSR1, SIGUSR2, }

enum CustomRepr {
    UserDefOne,
}

impl TryFrom<SignalNumber> for CustomRepr {
    type Error = ();

    fn try_from(value: SignalNumber) -> Result<Self, Self::Error> {
        match value {
            SIGUSR1 => Ok(Self::UserDefOne),
            _ => Err(()),
        }
    }
}

#[derive(Debug)]
struct CustomSender(mpsc::SyncSender<SignalNumber>);

impl Sender for CustomSender {
    fn send(&self, sig_num: SignalNumber) -> Result<(), SendError> {
        self.0.try_send(sig_num).map_err(|e| match e {
            mpsc::TrySendError::Full(_) => SendError::Full,
            mpsc::TrySendError::Disconnected(_) => SendError::Disconnected,
        })
    }
}


#[test]
#[allow(clippy::too_many_lines)]
fn main() {
    trait Recv<N> {
        fn recv(&self) -> N;
    }
    impl<N, S> Recv<N> for Receiver<N, S> {
        fn recv(&self) -> N { self.as_ref().recv().unwrap() }
    }

    assert!(SignalsChannel::is_finished());
    assert!(matches!(
        SignalsChannel::finish_with_outside_channel(),
        Err(FinishError::AlreadyFinished)
    ));
    assert!(SignalsChannel::is_finished());

    let receiver: Receiver<SignalNumber, _> = SignalsChannel::install(None).unwrap();
    assert!(SignalsChannel::is_installed());

    spawn_raise(SIGUSR1);
    assert_eq!(receiver.recv(), SIGUSR1);
    spawn_raise(SIGUSR2);
    assert_eq!(receiver.recv(), SIGUSR2);

    let r = SignalsChannel::uninstall_with_outside_channel();
    assert!(matches!(r, Err(UninstallError::WrongMethod)));
    assert!(SignalsChannel::is_installed());

    let additional = {
        // It's alright to have an additional (global, mutable) facility, but only if the sets of
        // signal numbers are disjoint.
        signals_receipts::channel_notify_facility! { mod additional { SIGURG } }

        additional::SignalsChannel::install::<SignalNumber>(Some(1)).unwrap()
    };
    spawn_raise(SIGURG);
    assert_eq!(additional.recv(), SIGURG);

    spawn_raise(SIGURG);
    spawn_raise(SIGUSR2);
    assert_eq!(receiver.recv(), SIGUSR2);
    spawn_raise(SIGURG);
    spawn_raise(SIGUSR1);
    assert_eq!(receiver.recv(), SIGUSR1);
    assert_eq!(additional.recv(), SIGURG);
    assert_eq!(additional.recv(), SIGURG);

    let r = SignalsChannel::uninstall(receiver);
    assert!(r.is_ok());
    assert!(SignalsChannel::is_dormant());
    spawn_raise(SIGURG);
    assert_eq!(additional.recv(), SIGURG);

    let receiver: Receiver<CustomRepr, _> = SignalsChannel::install(Some(0)).unwrap();
    assert!(SignalsChannel::is_installed());

    let r = SignalsChannel::finish_with_outside_channel();
    assert!(matches!(r, Err(FinishError::WrongMethod)));
    assert!(SignalsChannel::is_installed());

    assert!(matches!(
        SignalsChannel::install::<i32>(None),
        Err(InstallError::AlreadyInstalled { unused_notify: () })
    ));
    assert!(SignalsChannel::is_installed());

    spawn_raise(SIGUSR2); // Ignored and not sent - `try_from` fails for this.
    spawn_raise(SIGUSR1);
    assert!(matches!(receiver.recv(), CustomRepr::UserDefOne));
    assert!(matches!(receiver.as_ref().try_recv(), Err(TryRecvError::Empty)));
    spawn_raise(SIGUSR1);
    assert!(matches!(receiver.recv(), CustomRepr::UserDefOne));

    let r = SignalsChannel::finish(receiver);
    assert!(r.is_ok());
    assert!(SignalsChannel::is_finished());
    spawn_raise(SIGURG);
    assert_eq!(additional.recv(), SIGURG);

    let (sender, receiver) = mpsc::channel::<CustomRepr>();
    SignalsChannel::install_with_outside_channel(sender).unwrap();
    assert!(SignalsChannel::is_installed());
    {
        let (sender, receiver) = mpsc::channel::<u8>();
        let r = SignalsChannel::install_with_outside_channel(sender);
        if let Err(InstallError::AlreadyInstalled { unused_notify }) = r {
            unused_notify.send(123).unwrap();
            assert_eq!(receiver.try_recv().unwrap(), 123);
        } else {
            unreachable!();
        }
    }
    assert!(SignalsChannel::is_installed());
    let t = thread::spawn(move || receiver.recv());
    spawn_raise(SIGURG);
    spawn_raise(SIGUSR1);
    assert!(matches!(t.join().unwrap(), Ok(CustomRepr::UserDefOne)));
    assert_eq!(additional.recv(), SIGURG);

    let r = SignalsChannel::uninstall_with_outside_channel();
    assert!(r.is_ok());
    assert!(SignalsChannel::is_dormant());
    // `receiver` was already dropped (when `t` finished), as required by
    // `uninstall_with_outside_channel`.
    spawn_raise(SIGURG);
    assert_eq!(additional.recv(), SIGURG);

    let (sender, receiver) = mpsc::sync_channel(1);
    SignalsChannel::install_with_outside_channel(CustomSender(sender)).unwrap();
    assert!(SignalsChannel::is_installed());
    spawn_raise(SIGURG);
    spawn_raise(SIGUSR2);
    assert_eq!(receiver.recv().unwrap(), SIGUSR2);
    assert_eq!(additional.recv(), SIGURG);

    let r = SignalsChannel::finish_with_outside_channel();
    assert!(r.is_ok());
    assert!(SignalsChannel::is_finished());
    assert!(matches!(
        SignalsChannel::finish_with_outside_channel(),
        Err(FinishError::AlreadyFinished)
    ));
    assert!(SignalsChannel::is_finished());
    spawn_raise(SIGURG);
    assert_eq!(additional.recv(), SIGURG);
}


#[test]
fn without_commas() {
    signals_receipts::channel_notify_facility! { SIGALRM SIGCHLD SIGHUP SIGTTOU SIGXFSZ }
}
