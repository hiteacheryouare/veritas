use veritas_domain::{CoreError, Money};

pub(crate) fn to_money(value: i128) -> Result<Money, CoreError> {
	i64::try_from(value)
		.map(Money::from_minor)
		.map_err(|_| CoreError::Overflow)
}

/// `round_half_up(a / b)` for `a >= 0`: `(2a + b) / (2b)`. A negative `b`
/// moves its sign onto `a`, and a negative `a` rounds half away from zero.
pub(crate) fn round_half_up(a: i128, b: i128) -> i128 {
	round_half_away(a, b)
}

pub(crate) fn round_half_away(a: i128, b: i128) -> i128 {
	if b == 0 {
		return 0;
	}
	let (a, b) = if b < 0 { (-a, -b) } else { (a, b) };
	let magnitude = (2 * a.abs() + b) / (2 * b);
	if a < 0 { -magnitude } else { magnitude }
}

pub(crate) fn ceil_div(a: i128, b: i128) -> i128 {
	if b <= 0 {
		return 0;
	}
	(a + b - 1).div_euclid(b)
}

pub(crate) fn isqrt(value: i128) -> i128 {
	if value <= 0 {
		return 0;
	}
	let mut low = 0i128;
	let mut high = value.min(1 << 62) + 1;
	while high - low > 1 {
		let mid = low + (high - low) / 2;
		if mid.checked_mul(mid).is_some_and(|square| square <= value) {
			low = mid;
		} else {
			high = mid;
		}
	}
	low
}

/// FC-1's median: the middle value, or the two middle values averaged and
/// rounded half away from zero. `None` for no values.
pub(crate) fn median(values: &[i64]) -> Option<i64> {
	let mut sorted = values.to_vec();
	sorted.sort_unstable();
	let middle = sorted.len() / 2;
	if sorted.is_empty() {
		None
	} else if sorted.len() % 2 == 1 {
		sorted.get(middle).copied()
	} else {
		let low = i128::from(*sorted.get(middle - 1)?);
		let high = i128::from(*sorted.get(middle)?);
		i64::try_from(round_half_away(low + high, 2)).ok()
	}
}

/// Nearest rank: the `ceil(n * tenths / 10)`-th smallest value.
pub(crate) fn percentile(sorted: &[i64], tenths: usize) -> Option<i64> {
	let rank = (sorted.len() * tenths).div_ceil(10).max(1);
	sorted.get(rank - 1).copied()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn rounds_half_up_and_half_away_fc_0() {
		assert_eq!(round_half_up(5, 2), 3);
		assert_eq!(round_half_up(4, 2), 2);
		assert_eq!(round_half_away(-5, 2), -3);
		assert_eq!(round_half_away(-4, 3), -1);
		assert_eq!(round_half_away(5, -2), -3);
	}

	#[test]
	fn integer_square_root_truncates_fc_4() {
		assert_eq!(isqrt(100_000_000), 10_000);
		assert_eq!(isqrt(300_000_000), 17_320);
		assert_eq!(isqrt(400_000_000), 20_000);
		assert_eq!(isqrt(1_200_000_000), 34_641);
	}

	#[test]
	fn medians_follow_fc_1() {
		assert_eq!(median(&[42000, 42001, 41000, 45000]), Some(42001));
		assert_eq!(median(&[-3, -2]), Some(-3));
		assert_eq!(median(&[7]), Some(7));
		assert_eq!(median(&[]), None);
	}
}
