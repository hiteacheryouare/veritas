//! FC-7 backtest on synthetic history with known noise. The tables in
//! docs/forecast-eval.md are this test's output: it fails when they drift,
//! and `VERITAS_WRITE_FORECAST_EVAL=1` rewrites them.

#![allow(clippy::unwrap_used, clippy::panic)]

mod support;

use std::fmt::Write as _;
use std::path::PathBuf;

use support::*;
use veritas_forecast::{Backtest, Forecaster, Metrics};

const START: &str = "2024-01";
const MONTHS: i32 = 36;
const BEGIN: &str = "<!-- generated:start -->";
const END: &str = "<!-- generated:end -->";

/// SplitMix64, so the history is the same on every machine and nearby seeds
/// give unrelated streams (a raw xorshift seeded 1, 2, 3 starts out linear in
/// the seed).
struct Noise(u64);

impl Noise {
	fn next(&mut self) -> u64 {
		self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
		let mut z = self.0;
		z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
		z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
		z ^ (z >> 31)
	}

	/// Uniform in `-spread ..= spread`.
	fn around(&mut self, spread: i64) -> i64 {
		let width = (2 * spread + 1) as u64;
		(self.next() % width) as i64 - spread
	}

	fn chance(&mut self, percent: u64) -> bool {
		self.next() % 100 < percent
	}
}

#[derive(Clone, Copy, PartialEq)]
enum Mix {
	Stationary,
	Seasonal,
	Household,
}

/// Electric's winter and summer peaks, in percent of a typical month.
const ELECTRIC_SEASON: [i64; 12] = [150, 140, 100, 80, 80, 110, 130, 130, 100, 80, 90, 140];

fn history(mix: Mix, seed: u64) -> Book {
	let mut noise = Noise(seed);
	let mut book = Book::household();
	book.opening("CHK", "2023-12-31", 2_000_000);
	for step in 0..MONTHS {
		let month = ym(START).offset(step).unwrap();
		let day = |day: u32| format!("{month}-{day:02}");
		if mix != Mix::Seasonal {
			for (week, spread) in [(3, 1500), (10, 1500), (17, 1500), (24, 1500)] {
				let x = 10750 + noise.around(spread);
				book.row("VISA", &day(week), -x).in_category("Groceries");
			}
			let dining = 15000 + noise.around(6000);
			book.row("VISA", &day(12), -dining).in_category("Dining");
		}
		if mix != Mix::Stationary {
			let season = ELECTRIC_SEASON[(month.month() - 1) as usize];
			let electric = 9000 * season / 100 + noise.around(800);
			book.spend("Electric", &day(20), -electric);
			let gifts = if month.month() == 12 {
				40000 + noise.around(8000)
			} else {
				2000 + noise.around(1500)
			};
			book.row("VISA", &day(18), -gifts).in_category("Gifts");
		}
		if mix == Mix::Household {
			book.spend("Paycheck", &day(1), 300000).bill("Paycheck");
			book.spend("Rent", &day(1), -165000).bill("Rent");
			if noise.chance(30) {
				book.spend("Side gigs", &day(25), 30000 + noise.around(10000));
			}
			book.transfer("CHK", "VISA", &day(28), -60000);
		}
	}
	book
}

const SEEDS: u64 = 20;

fn run(mix: Mix, seed: u64, seasonal: bool) -> Backtest {
	let book = history(mix, seed);
	let forecaster = Forecaster::new(&book.budget).unwrap();
	let forecaster = if seasonal {
		forecaster
	} else {
		forecaster.without_seasonality()
	};
	forecaster
		.backtest(ym(START).offset(MONTHS).unwrap())
		.unwrap()
}

fn dollars(minor: i64) -> String {
	let sign = if minor < 0 { "-" } else { "" };
	format!("{sign}${}.{:02}", minor.abs() / 100, minor.abs() % 100)
}

fn bp(value: i64) -> String {
	let sign = if value < 0 { "-" } else { "" };
	format!("{sign}{}.{:02}%", value.abs() / 100, value.abs() % 100)
}

/// The median of whole numbers, the lower middle for an even count, which
/// is a reported value rather than a computed one.
fn middle(values: &mut [i64]) -> i64 {
	values.sort_unstable();
	values[(values.len() - 1) / 2]
}

/// "median (min to max)" over seeds, or "n/a" when no seed had a value.
fn spread(values: &[Option<i64>], show: fn(i64) -> String) -> String {
	let mut present: Vec<i64> = values.iter().flatten().copied().collect();
	if present.is_empty() {
		return "n/a".to_owned();
	}
	let mid = middle(&mut present);
	format!(
		"{} ({} to {})",
		show(mid),
		show(present[0]),
		show(present[present.len() - 1])
	)
}

fn table(out: &mut String, title: &str, on: &[Vec<Metrics>], off: &[Vec<Metrics>]) {
	writeln!(out, "**{title}**\n").unwrap();
	writeln!(
		out,
		"| h | n | MAE | WAPE | Bias | Band coverage | Skill vs last month | Seeds below 0 | Skill vs last year | Skill vs last month, seasonality off |"
	)
	.unwrap();
	writeln!(out, "|---|---|---|---|---|---|---|---|---|---|").unwrap();
	let horizons: Vec<u32> = on[0].iter().map(|row| row.horizon).collect();
	for (slot, horizon) in horizons.iter().enumerate() {
		let rows: Vec<&Metrics> = on.iter().map(|seed| &seed[slot]).collect();
		let off_rows: Vec<&Metrics> = off.iter().map(|seed| &seed[slot]).collect();
		let pick = |rows: &[&Metrics], get: fn(&Metrics) -> Option<i64>| -> Vec<Option<i64>> {
			rows.iter().map(|row| get(row)).collect()
		};
		let below = rows
			.iter()
			.filter(|row| row.skill_vs_last_month.is_some_and(|skill| skill < 0))
			.count();
		writeln!(
			out,
			"| {horizon} | {} | {} | {} | {} | {} | {} | {below} of {} | {} | {} |",
			rows[0].n,
			spread(&pick(&rows, |row| Some(row.mae.minor())), dollars),
			spread(&pick(&rows, |row| row.wape), bp),
			spread(&pick(&rows, |row| Some(row.bias.minor())), dollars),
			spread(&pick(&rows, |row| row.coverage), |value| format!(
				"{value}%"
			)),
			spread(&pick(&rows, |row| row.skill_vs_last_month), bp),
			rows.len(),
			spread(&pick(&rows, |row| row.skill_vs_last_year), bp),
			spread(&pick(&off_rows, |row| row.skill_vs_last_month), bp),
		)
		.unwrap();
	}
	out.push('\n');
}

fn generated() -> String {
	let mut out = String::new();
	for (mix, name) in [
		(Mix::Stationary, "Stationary: Groceries and Dining"),
		(Mix::Seasonal, "Seasonal: Electric and Gifts"),
		(
			Mix::Household,
			"Household: all of the above, plus bills, pay, side income and card payments",
		),
	] {
		let on: Vec<Backtest> = (1..=SEEDS).map(|seed| run(mix, seed, true)).collect();
		let off: Vec<Backtest> = (1..=SEEDS).map(|seed| run(mix, seed, false)).collect();
		writeln!(out, "### {name}\n").unwrap();
		let flow = |runs: &[Backtest]| {
			runs.iter()
				.map(|run| run.uncertain_flow.clone())
				.collect::<Vec<_>>()
		};
		let worth = |runs: &[Backtest]| {
			runs.iter()
				.map(|run| run.net_worth.clone())
				.collect::<Vec<_>>()
		};
		table(
			&mut out,
			"(a) Baselined net flow of month o + h",
			&flow(&on),
			&flow(&off),
		);
		table(
			&mut out,
			"(b) Net-worth change over o + 1 to o + h, non-cash changes excluded, bills as perfect foresight",
			&worth(&on),
			&worth(&off),
		);
	}
	out
}

#[test]
fn fc_7_backtest_metrics_match_docs_forecast_eval() {
	let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/forecast-eval.md");
	// .gitattributes checks Markdown out with CRLF, so compare without it.
	let raw = std::fs::read_to_string(&path).unwrap();
	let document = raw.replace("\r\n", "\n");
	let (head, rest) = document.split_once(BEGIN).unwrap();
	let (_, tail) = rest.split_once(END).unwrap();
	let expected = format!("{head}{BEGIN}\n\n{}{END}{tail}", generated());
	if std::env::var_os("VERITAS_WRITE_FORECAST_EVAL").is_some() {
		let expected = if raw.contains("\r\n") {
			expected.replace('\n', "\r\n")
		} else {
			expected
		};
		std::fs::write(&path, expected).unwrap();
		return;
	}
	assert!(
		document == expected,
		"docs/forecast-eval.md is out of date: rerun with VERITAS_WRITE_FORECAST_EVAL=1"
	);
}

#[test]
fn fc_7_every_horizon_is_scored_on_24_months_or_more() {
	let backtest = run(Mix::Household, 1, true);
	let horizons: Vec<u32> = backtest
		.uncertain_flow
		.iter()
		.map(|row| row.horizon)
		.collect();
	assert_eq!(horizons, [1, 3, 6, 10, 12]);
	assert_eq!(backtest.uncertain_flow[0].n, MONTHS as usize - 3 - 1);
	assert_eq!(backtest.net_worth[0].n, MONTHS as usize - 6 - 1);
	assert!(backtest.net_worth.iter().all(|row| row.n > 0));
}
