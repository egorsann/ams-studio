//! Small helpers shared by the windows.

/// `clamp` that does not panic when the bounds are the wrong way round (a window smaller than its minimum size, a pane
/// squeezed to nothing) or NaN: an inverted pair gives the lower bound, NaN bounds leave the value alone. The standard
/// `clamp` panics in those cases, which closes the program.
pub trait SafeClamp: Sized + PartialOrd + Copy {
    fn clamp_safe(self, lo: Self, hi: Self) -> Self {
        // a NaN bound (it is not equal to itself) leaves the value unchanged; bounds the wrong way round give the lower one
        if lo != lo || hi != hi {
            return self;
        }
        if lo > hi {
            return lo;
        }
        if self < lo {
            lo
        } else if self > hi {
            hi
        } else {
            self
        }
    }
}

impl SafeClamp for f32 {}
impl SafeClamp for f64 {}
impl SafeClamp for usize {}
impl SafeClamp for i64 {}
impl SafeClamp for u32 {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrong_way_round_bounds_do_not_panic() {
        assert_eq!(5.0f32.clamp_safe(180.0, 86.0), 180.0);
        assert_eq!(5.0f32.clamp_safe(1.0, 9.0), 5.0);
        assert_eq!(50.0f32.clamp_safe(1.0, 9.0), 9.0);
        assert_eq!(5.0f32.clamp_safe(f32::NAN, 9.0), 5.0);
        assert_eq!(5.0f32.clamp_safe(1.0, f32::NAN), 5.0);
        assert_eq!(7usize.clamp_safe(3, 0), 3);
        assert_eq!(f32::NAN.clamp_safe(1.0, 2.0).is_nan(), true);
    }
}
