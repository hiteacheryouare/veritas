//! `Math.log` and `Math.cos` as V8 computes them, so the Box–Muller draws
//! match Actual's to the bit. V8 runs fdlibm (`src/base/ieee754.cc`), and
//! clang builds it for arm64 with each `a * b + c` fused into one rounding,
//! the left multiply when both sides are products. The `mul_add` calls
//! below sit exactly where clang fuses; the platform `ln` and `cos` miss
//! V8 by an ulp on roughly one draw in twenty. Checked bit for bit against
//! Node 26 on arm64 over two million Box–Muller inputs and half a million
//! edge cases. V8 on x86-64 doesn't fuse, so its last bits differ there.

fn high(x: f64) -> i32 {
	(x.to_bits() >> 32) as u32 as i32
}

fn words(high: u32, low: u32) -> f64 {
	f64::from_bits((u64::from(high) << 32) | u64::from(low))
}

fn with_high(x: f64, high: i32) -> f64 {
	f64::from_bits((u64::from(high as u32) << 32) | (x.to_bits() & 0xffff_ffff))
}

/// `__ieee754_log`.
pub(crate) fn log(x: f64) -> f64 {
	let ln2_hi = words(0x3fe6_2e42, 0xfee0_0000);
	let ln2_lo = words(0x3dea_39ef, 0x3579_3c76);
	let lg = [
		words(0x3fe5_5555, 0x5555_5593),
		words(0x3fd9_9999, 0x9997_fa04),
		words(0x3fd2_4924, 0x9422_9359),
		words(0x3fcc_71c5, 0x1d8e_78af),
		words(0x3fc7_4664, 0x96cb_03de),
		words(0x3fc3_9a09, 0xd078_c69f),
		words(0x3fc2_f112, 0xdf3e_5244),
	];
	let mut x = x;
	let mut hx = high(x);
	let mut k = 0;
	if hx < 0x0010_0000 {
		if x == 0.0 {
			return f64::NEG_INFINITY;
		}
		if hx < 0 {
			return f64::NAN;
		}
		k -= 54;
		x *= words(0x4350_0000, 0);
		hx = high(x);
	}
	if hx >= 0x7ff0_0000 {
		return x + x;
	}
	k += (hx >> 20) - 1023;
	hx &= 0x000f_ffff;
	let i = (hx + 0x95f64) & 0x0010_0000;
	x = with_high(x, hx | (i ^ 0x3ff0_0000));
	k += i >> 20;
	let f = x - 1.0;
	let dk = f64::from(k);
	if (0x000f_ffff & (2 + hx)) < 3 {
		if f == 0.0 {
			return if k == 0 {
				0.0
			} else {
				dk.mul_add(ln2_hi, dk * ln2_lo)
			};
		}
		let r = f * f * (-0.333_333_333_333_333_3_f64).mul_add(f, 0.5);
		return if k == 0 {
			f - r
		} else {
			dk.mul_add(ln2_hi, -((-dk).mul_add(ln2_lo, r) - f))
		};
	}
	let s = f / (2.0 + f);
	let z = s * s;
	let w = z * z;
	let t1 = w * w.mul_add(w.mul_add(lg[5], lg[3]), lg[1]);
	let t2 = z * w.mul_add(w.mul_add(w.mul_add(lg[6], lg[4]), lg[2]), lg[0]);
	let r = t2 + t1;
	if ((hx - 0x6147a) | (0x6b851 - hx)) > 0 {
		let hfsq = 0.5 * f * f;
		if k == 0 {
			f - (-s).mul_add(hfsq + r, hfsq)
		} else {
			dk.mul_add(ln2_hi, -((hfsq - s.mul_add(hfsq + r, dk * ln2_lo)) - f))
		}
	} else if k == 0 {
		(-s).mul_add(f - r, f)
	} else {
		dk.mul_add(ln2_hi, -(s.mul_add(f - r, -(dk * ln2_lo)) - f))
	}
}

/// `__kernel_cos` on `x + y`, |x| ≤ π/4.
fn kernel_cos(x: f64, y: f64) -> f64 {
	let c = [
		words(0x3fa5_5555, 0x5555_554c),
		words(0xbf56_c16c, 0x16c1_5177),
		words(0x3efa_01a0, 0x19cb_1590),
		words(0xbe92_7e4f, 0x809c_52ad),
		words(0x3e21_ee9e, 0xbdb4_b1c4),
		words(0xbda8_fae9, 0xbe88_38d4),
	];
	let ix = high(x) & 0x7fff_ffff;
	if ix < 0x3e40_0000 && x as i32 == 0 {
		return 1.0;
	}
	let z = x * x;
	let r = z * z.mul_add(
		z.mul_add(
			z.mul_add(z.mul_add(z.mul_add(c[5], c[4]), c[3]), c[2]),
			c[1],
		),
		c[0],
	);
	let zr = z.mul_add(r, -(x * y));
	if ix < 0x3fd3_3333 {
		return 1.0 - 0.5_f64.mul_add(z, -zr);
	}
	let qx = if ix > 0x3fe9_0000 {
		0.28125
	} else {
		words((ix - 0x0020_0000) as u32, 0)
	};
	(1.0 - qx) - (0.5_f64.mul_add(z, -qx) - zr)
}

/// `__kernel_sin` on `x + y` with a tail, |x| ≤ π/4.
fn kernel_sin(x: f64, y: f64) -> f64 {
	let s = [
		words(0xbfc5_5555, 0x5555_5549),
		words(0x3f81_1111, 0x1110_f8a6),
		words(0xbf2a_01a0, 0x19c1_61d5),
		words(0x3ec7_1de3, 0x57b1_fe7d),
		words(0xbe5a_e5e6, 0x8a2b_9ceb),
		words(0x3de5_d93a, 0x5acf_d57c),
	];
	let ix = high(x) & 0x7fff_ffff;
	if ix < 0x3e40_0000 && x as i32 == 0 {
		return x;
	}
	let z = x * x;
	let v = z * x;
	let r = z.mul_add(
		z.mul_add(z.mul_add(z.mul_add(s[5], s[4]), s[3]), s[2]),
		s[1],
	);
	x - (-v).mul_add(s[0], z.mul_add(0.5_f64.mul_add(y, -(v * r)), -y))
}

/// The high words of n·π/2 for n = 1..=32, where the first reduction
/// step might cancel.
const PIO2_HIGH: [i32; 32] = [
	0x3ff9_21fb,
	0x4009_21fb,
	0x4012_d97c,
	0x4019_21fb,
	0x401f_6a7a,
	0x4022_d97c,
	0x4025_fdbb,
	0x4029_21fb,
	0x402c_463a,
	0x402f_6a7a,
	0x4031_475c,
	0x4032_d97c,
	0x4034_6b9c,
	0x4035_fdbb,
	0x4037_8fdb,
	0x4039_21fb,
	0x403a_b41b,
	0x403c_463a,
	0x403d_d85a,
	0x403f_6a7a,
	0x4040_7e4c,
	0x4041_475c,
	0x4042_106c,
	0x4042_d97c,
	0x4043_a28c,
	0x4044_6b9c,
	0x4045_34ac,
	0x4045_fdbb,
	0x4046_c6cb,
	0x4047_8fdb,
	0x4048_58eb,
	0x4049_21fb,
];

/// `__ieee754_rem_pio2` for π/4 < |x| ≤ 2¹⁹·π/2: the quadrant and the
/// remainder as a head and tail.
fn rem_pio2(x: f64) -> (i32, f64, f64) {
	let pio2_1 = words(0x3ff9_21fb, 0x5440_0000);
	let pio2_1t = words(0x3dd0_b461, 0x1a62_6331);
	let pio2_2 = words(0x3dd0_b461, 0x1a60_0000);
	let pio2_2t = words(0x3ba3_198a, 0x2e03_7073);
	let pio2_3 = words(0x3ba3_198a, 0x2e00_0000);
	let pio2_3t = words(0x397b_839a, 0x2520_49c1);
	let hx = high(x);
	let ix = hx & 0x7fff_ffff;
	if ix < 0x4002_d97c {
		let (sign, n) = if hx > 0 { (1.0, 1) } else { (-1.0, -1) };
		let z = x - sign * pio2_1;
		let (z, tail) = if ix == 0x3ff9_21fb {
			(z - sign * pio2_2, sign * pio2_2t)
		} else {
			(z, sign * pio2_1t)
		};
		let head = z - tail;
		return (n, head, (z - head) - tail);
	}
	let t = x.abs();
	let n = t.mul_add(words(0x3fe4_5f30, 0x6dc9_c883), 0.5) as i32;
	let fn_ = f64::from(n);
	let mut r = (-fn_).mul_add(pio2_1, t);
	let mut w = fn_ * pio2_1t;
	let mut head = r - w;
	if n >= 32 || ix == PIO2_HIGH[(n - 1) as usize] {
		let exponent = ix >> 20;
		let lost = |head: f64| exponent - ((high(head) >> 20) & 0x7ff);
		if lost(head) > 16 {
			let t = r;
			w = fn_ * pio2_2;
			r = t - w;
			w = fn_.mul_add(pio2_2t, -((t - r) - w));
			head = r - w;
			if lost(head) > 49 {
				let t = r;
				w = fn_ * pio2_3;
				r = t - w;
				w = fn_.mul_add(pio2_3t, -((t - r) - w));
				head = r - w;
			}
		}
	}
	let tail = (r - head) - w;
	if hx < 0 {
		(-n, -head, -tail)
	} else {
		(n, head, tail)
	}
}

/// `cos`. Box–Muller only asks for 0 ≤ x < 2π; past 2¹⁹·π/2, where fdlibm
/// switches to its long reduction, this falls back to the platform `cos`.
pub(crate) fn cos(x: f64) -> f64 {
	let ix = high(x) & 0x7fff_ffff;
	if ix <= 0x3fe9_21fb {
		return kernel_cos(x, 0.0);
	}
	if ix >= 0x7ff0_0000 {
		return f64::NAN;
	}
	if ix > 0x4139_21fb {
		return x.cos();
	}
	let (n, head, tail) = rem_pio2(x);
	match n & 3 {
		0 => kernel_cos(head, tail),
		1 => -kernel_sin(head, tail),
		2 => -kernel_cos(head, tail),
		_ => kernel_sin(head, tail),
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	/// Node 26 on arm64: `Math.log` and `Math.cos` of each input, as bits.
	#[test]
	fn matches_v8_on_awkward_inputs() {
		let cases: [(f64, u64, u64); 9] = [
			(0.5, 0xbfe6_2e42_fefa_39ef, 0x3fec_1528_065b_7d50),
			(
				2.328_306_436_538_696_3e-10,
				0xc036_2e42_fefa_39ef,
				0x3ff0_0000_0000_0000,
			),
			(
				0.999_999_999_767_169_4,
				0xbdf0_0000_0008_0000,
				0x3fe1_4a28_0fcf_f3e0,
			),
			(
				std::f64::consts::PI,
				0x3ff2_50d0_48e7_a1bd,
				0xbff0_0000_0000_0000,
			),
			(
				4.712_388_980_384_69,
				0x3ff8_cd99_4417_87d0,
				0xbcaa_7939_4c9e_8a0a,
			),
			(1e-300, 0xc085_9634_47f8_7fb5, 0x3ff0_0000_0000_0000),
			(
				std::f64::consts::FRAC_PI_4,
				0xbfce_eb95_b094_c193,
				0x3fe6_a09e_667f_3bcd,
			),
			(
				2.356_194_490_192_345,
				0x3feb_6cef_8934_d5b0,
				0xbfe6_a09e_667f_3bcc,
			),
			(5.2, 0x3ffa_60e7_ddf2_a866, 0x3fdd_fc2d_5937_6aee),
		];
		for (x, ln, cosine) in cases {
			assert_eq!(log(x).to_bits(), ln, "log {x}");
			assert_eq!(cos(x).to_bits(), cosine, "cos {x}");
		}
	}
}
