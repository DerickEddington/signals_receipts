use core::{hint,
           ops::Add,
           sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8,
                          Ordering::{self, Relaxed}}};


/// An unsigned integer with atomic operations as needed by this crate.
///
/// All implementations of these methods must be async-signal-safe, because they're called from
/// within an async-signal handler in an interrupt context.
///
/// This is already implemented for the standard atomic types.  If implemented for another type,
/// that type must really have lock-free atomic operations.
pub trait AtomicUInt: Default + 'static {
    /// The corresponding primitive unsigned integer type, with the needed operations.
    type UInt: Add<Output = Self::UInt> + Copy + Eq + From<u8>;
    /// The largest value that can be represented by [`Self::UInt`].
    const MAX: Self::UInt;

    /// Like [`Atomic*::load` et al](`AtomicU64::load`).
    #[must_use]
    fn load(&self, order: Ordering) -> Self::UInt;

    /// Like [`Atomic*::swap` et al](`AtomicU64::swap`).
    #[must_use]
    fn swap(&self, val: Self::UInt, order: Ordering) -> Self::UInt;

    /// Like [`Atomic*::compare_exchange` et al](`AtomicU64::compare_exchange`).
    #[allow(clippy::missing_errors_doc)]
    fn compare_exchange(
        &self,
        current: Self::UInt,
        new: Self::UInt,
        success: Ordering,
        failure: Ordering,
    ) -> Result<Self::UInt, Self::UInt>;

    /// Like [`Atomic*::fetch_add` et al](`AtomicU64::fetch_add`) of `1, Relaxed`, but saturates
    /// at the numeric bounds instead of overflowing, and returns the new value.
    #[inline]
    fn saturating_incr(&self) -> Self::UInt {
        let mut cur = self.load(Relaxed);
        loop {
            if cur == Self::MAX {
                break cur;
            } else {
                #[allow(clippy::arithmetic_side_effects)]
                let incr = cur + 1.into(); // (Can't overflow.)
                match self.compare_exchange(cur, incr, Relaxed, Relaxed) {
                    Ok(_) => break incr,
                    Err(latest) => {
                        cur = latest;
                        hint::spin_loop();
                    },
                }
            }
        }
    }
}


macro_rules! uints_impls {
        { ($t:ty, $u:ty) } => {
            impl AtomicUInt for $t {
                type UInt = $u;
                const MAX: Self::UInt = <$u>::MAX;

                #[inline]
                fn load(&self, order: Ordering) -> Self::UInt {
                    <$t>::load(self, order)
                }

                #[inline]
                fn swap(&self, val: Self::UInt, order: Ordering) -> Self::UInt {
                    <$t>::swap(self, val, order)
                }

                #[inline]
                fn compare_exchange(
                    &self,
                    current: Self::UInt,
                    new: Self::UInt,
                    success: Ordering,
                    failure: Ordering,
                ) -> Result<Self::UInt, Self::UInt> {
                    <$t>::compare_exchange(self, current, new, success, failure)
                }
            }
        };
        { $( ($t:ty, $u:ty); )+ } => {
            $( uints_impls! { ($t, $u) } )+
        };
    }

uints_impls! { (AtomicU8, u8); (AtomicU16, u16); (AtomicU32, u32); (AtomicU64, u64); }


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        let a1 = AtomicU64::new(1);
        assert_eq!(a1.saturating_incr(), 2);
        let a2 = AtomicU8::new(u8::MAX);
        assert_eq!(a2.saturating_incr(), u8::MAX);
    }
}
