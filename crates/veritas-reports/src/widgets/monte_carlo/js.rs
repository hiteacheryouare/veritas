//! The JavaScript arithmetic the simulation leans on, kept bit for bit:
//! its random stream, `Math.min`/`Math.max`, and both kinds of sort.

use std::cmp::Ordering;
use std::f64::consts::PI;

use super::fdlibm;

pub(crate) use veritas_engine::js_round;

/// `mulberry32` (`monteCarloSimulation.ts:1218-1227`). `Math.imul` and the
/// `| 0` and `>>> 0` coercions are all arithmetic mod 2³², so `u32`
/// wrapping operations give the same bits.
pub(crate) struct Mulberry32 {
	state: u32,
}

impl Mulberry32 {
	pub(crate) fn new(seed: u32) -> Self {
		Mulberry32 { state: seed }
	}

	pub(crate) fn next(&mut self) -> f64 {
		self.state = self.state.wrapping_add(0x6d2b_79f5);
		let state = self.state;
		let mut mixed = (state ^ (state >> 15)).wrapping_mul(1 | state);
		mixed = mixed.wrapping_add((mixed ^ (mixed >> 7)).wrapping_mul(61 | mixed)) ^ mixed;
		f64::from(mixed ^ (mixed >> 14)) / 4_294_967_296.0
	}

	/// `makeNormalSampler`'s Box–Muller draw (`monteCarloSimulation.ts:1230-1239`),
	/// with V8's own `log` and `cos`.
	pub(crate) fn next_normal(&mut self) -> f64 {
		let uniform1 = 1.0 - self.next();
		let uniform2 = self.next();
		(-2.0 * fdlibm::log(uniform1)).sqrt() * fdlibm::cos(2.0 * PI * uniform2)
	}
}

/// `Math.max`: NaN wins, and +0 beats −0.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() {
		f64::NAN
	} else if a > b || (a == b && a.is_sign_positive()) {
		a
	} else {
		b
	}
}

/// `Math.min`: NaN wins, and −0 beats +0.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
	if a.is_nan() || b.is_nan() {
		f64::NAN
	} else if a < b || (a == b && a.is_sign_negative()) {
		a
	} else {
		b
	}
}

/// The simulation's `clamp` (`monteCarloSimulation.ts:1213-1215`).
pub(crate) fn clamp(value: f64, min: f64, max: f64) -> f64 {
	js_min(max, js_max(min, value))
}

/// `Array.prototype.sort` with a numeric comparator: stable, and a NaN
/// comparison counts as a tie. The lists are a handful of pots or bands.
pub(crate) fn stable_sort<T>(items: &mut [T], compare: impl Fn(&T, &T) -> f64) {
	for index in 1..items.len() {
		let mut at = index;
		while at > 0 && compare(&items[at - 1], &items[at]) > 0.0 {
			items.swap(at - 1, at);
			at -= 1;
		}
	}
}

/// `Float64Array.prototype.sort()`: ascending, −0 before +0, NaN last.
pub(crate) fn sort_typed(values: &mut [f64]) {
	values.sort_unstable_by(|a, b| match (a.is_nan(), b.is_nan()) {
		(true, true) => Ordering::Equal,
		(true, false) => Ordering::Greater,
		(false, true) => Ordering::Less,
		(false, false) => a.total_cmp(b),
	});
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn mulberry32_matches_javascript() {
		// The first draws of mulberry32(1234), printed by Node.
		let mut random = Mulberry32::new(1234);
		let draws: Vec<f64> = (0..3).map(|_| random.next()).collect();
		assert_eq!(
			draws,
			[0.07329497812315822, 0.7034119898453355, 0.9028560190927237]
		);
	}

	#[test]
	fn min_and_max_follow_javascript_for_zeros_and_nan() {
		assert!(js_max(-0.0, 0.0).is_sign_positive());
		assert!(js_min(0.0, -0.0).is_sign_negative());
		assert!(js_max(1.0, f64::NAN).is_nan());
		assert!(clamp(f64::NAN, 0.0, 1.0).is_nan());
	}

	#[test]
	fn stable_sort_keeps_ties_in_order() {
		let mut items = [(1, 'a'), (0, 'b'), (1, 'c'), (0, 'd')];
		stable_sort(&mut items, |a, b| f64::from(a.0 - b.0));
		assert_eq!(items, [(0, 'b'), (0, 'd'), (1, 'a'), (1, 'c')]);
	}
}
