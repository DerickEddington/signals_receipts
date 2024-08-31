use crate::SignalNumber;
use core::{cmp::Ordering, mem, ops::ControlFlow};


/// Representation of receipt of delivery of a signal, as given to delegates declared in uses of
/// the [`premade`](crate::premade!) macro or delegates given to the premade
/// [`consume_count_then_delegate`](crate::consume_count_then_delegate) helper (which is
/// automatically used by the `premade` macro).
///
/// `B` is the type of the final value that the processing finishes with.  `C` is the type of the
/// state value that is passed in and out of all delegates during processing.
#[non_exhaustive]
#[must_use]
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub struct Receipt<U, B = (), C = ()> {
    /// Signal number that was received.
    pub sig_num:   SignalNumber,
    /// Current count of how many times the signal designated by `sig_num` was received since
    /// last time its consuming was run.
    pub cur_count: U,
    /// Control whether the processing of subsequent receipts will continue or finish after the
    /// current delegate (which is processing this instance) returns.
    ///
    /// Initialized to `Continue` with the current state value, so that continuing is what
    /// is done by default.  Intended to be mutated to `Break` by a delegate that wants to cause
    /// finishing.
    ///
    /// When `Continue`, the contained value can be used as a mutable state that is passed
    /// to and returned from all delegates during processing.
    pub flow:      ControlFlow<B, C>,
}


impl<U, B, C> Receipt<U, B, C> {
    const NOT_STATE_MSG: &'static str = "should be `ControlFlow::Continue` to get state";

    /// Cause the processing to finish.
    ///
    /// Assigns `self.flow = ControlFlow::Break(B::default())`.
    #[inline]
    pub fn break_loop(&mut self)
    where
        B: Default,
    {
        self.break_loop_with(B::default());
    }

    /// Cause the processing to finish with the given value.
    ///
    /// Assigns `self.flow = ControlFlow::Break(val)`.
    #[inline]
    pub fn break_loop_with(&mut self, val: B) { self.flow = ControlFlow::Break(val); }

    /// Return a reference to the state value (which is held in `self.flow`).
    ///
    /// # Panics
    /// If `self.flow` is not `ControlFlow::Continue`.  When a `Receipt` is given to a delegate,
    /// it is guaranteed to hold `Continue`, and so this won't panic.
    #[must_use]
    #[inline]
    pub fn get_state_ref(&self) -> &C {
        match &self.flow {
            ControlFlow::Continue(state) => state,
            #[allow(clippy::panic)]
            ControlFlow::Break(_) => panic!("{}", Self::NOT_STATE_MSG),
        }
    }

    /// Like [`Self::get_state_ref`] but returns a mutable reference.
    ///
    /// # Panics
    /// Same as `Self::get_state_ref`.
    #[must_use]
    #[inline]
    pub fn get_state_mut(&mut self) -> &mut C {
        match &mut self.flow {
            ControlFlow::Continue(state) => state,
            #[allow(clippy::panic)]
            ControlFlow::Break(_) => panic!("{}", Self::NOT_STATE_MSG),
        }
    }

    /// Cause the processing to continue with the given state value.
    ///
    /// Assigns `self.flow = ControlFlow::Continue(val)`.
    #[inline]
    pub fn set_state(&mut self, val: C) { self.flow = ControlFlow::Continue(val); }

    /// Apply the given `updater` function to the state value as mutable, to update it.
    ///
    /// The `updater` can mutate or replace the value in-place.
    ///
    /// # Panics
    /// Same as [`Self::get_state_mut`].
    #[inline]
    pub fn update_state<F: FnOnce(&mut C)>(&mut self, updater: F) {
        updater(self.get_state_mut());
    }

    /// Replace the state with the given `val` and return the previous value.
    ///
    /// # Panics
    /// Same as [`Self::get_state_mut`].
    #[must_use]
    #[inline]
    pub fn replace_state(&mut self, val: C) -> C { mem::replace(self.get_state_mut(), val) }

    /// Return the current state value and replace it with the default.
    ///
    /// # Panics
    /// Same as [`Self::get_state_mut`].
    #[must_use]
    #[inline]
    pub fn take_state(&mut self) -> C
    where
        C: Default,
    {
        mem::take(self.get_state_mut())
    }
}


/// Manual impl because [`ControlFlow`] doesn't impl `PartialOrd`.
impl<U: Ord, B: Ord, C: Ord> PartialOrd for Receipt<U, B, C> {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

/// Manual impl because [`ControlFlow`] doesn't impl `Ord`.
impl<U: Ord, B: Ord, C: Ord> Ord for Receipt<U, B, C> {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        match self.sig_num.cmp(&other.sig_num) {
            Ordering::Equal => match self.cur_count.cmp(&other.cur_count) {
                Ordering::Equal => match (&self.flow, &other.flow) {
                    (ControlFlow::Continue(c1), ControlFlow::Continue(c2)) => c1.cmp(c2),
                    (ControlFlow::Continue(_), ControlFlow::Break(_)) => Ordering::Less,
                    (ControlFlow::Break(_), ControlFlow::Continue(_)) => Ordering::Greater,
                    (ControlFlow::Break(b1), ControlFlow::Break(b2)) => b1.cmp(b2),
                },
                ord @ (Ordering::Less | Ordering::Greater) => ord,
            },
            ord @ (Ordering::Less | Ordering::Greater) => ord,
        }
    }
}
