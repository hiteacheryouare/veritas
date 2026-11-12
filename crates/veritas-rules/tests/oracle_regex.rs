//! R-RULE-1 and R-RULE-3 for `matches` conditions (D47), against the oracle.
//! Each pattern becomes its own rule that appends a marker to the notes, so
//! the notes Actual stores say exactly which patterns matched each
//! transaction. Veritas must give the same answer for every pair, or report
//! the rule as undetermined. A wrong match or a wrong miss is never allowed.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::collections::{BTreeMap, BTreeSet};

use common::{
	Rng, apply_and_dump, oracle_dir, repo_root, rule_from_row, rule_transaction, scratch_dir, uuid,
};
use serde_json::{Value, json};
use veritas_domain::{Account, AccountId, BudgetData, BudgetType, Payee, PayeeId, RuleId};
use veritas_rules::{BudgetPayees, ParsedRule, RuleBook, RuleSupport};

const TODAY: &str = "2026-11-09";
const GENERATED_PATTERNS: usize = 360;
const GENERATED_TEXTS: usize = 40;

/// Rules people write to clean up bank descriptors.
const BANK_PATTERNS: &[&str] = &[
	"^amzn mktp",
	"amazon|amzn",
	"^AMZN Mktp US\\*\\w+",
	"^sq \\*",
	"^tst\\* ",
	"uber\\s*(eats|trip)",
	"^paypal \\*",
	"#\\d{3,5}",
	"\\d{4}$",
	"^ach (credit|debit)",
	"venmo.*(cashout|payment)",
	"^pos debit",
	"^(?:wal-?mart|wm supercenter)",
	"costco (whse|gas)",
	"\\b(shell|chevron|exxon)\\b",
	"^check \\d+$",
	"^atm withdrawal",
	"zelle (to|from) ",
	"\\$\\d+\\.\\d\\d",
	"spotify|netflix|hulu",
	"apple\\.com/bill",
	"^ppd id:",
	"^[a-z]+ [a-z]+$",
	"^\\s*$",
	".*",
	"^.*$",
	".+",
	"[A-Z]{2,}",
	"^\\D+$",
	"\\W",
	"^[\\w .'&-]+$",
	"[\\w.-]+@[\\w-]+\\.\\w+",
	"(?<store>\\d{3,})$",
	"caf[eé]",
	"école|ecole",
	"straße|strasse",
	"İstanbul",
	"istanbul",
	"οδος",
	"ΟΔΟΣ$",
	"\\u00e9",
	"\\x41",
	"[^\\x00-\\x7f]",
	"^[\\x20-\\x7e]+$",
	"\\s{2,}",
	"line1\\r?\\nline2",
	"^line1.line2$",
	"line1[^x]+line2",
	"nbsp\\s",
	"\\u212a",
	"kelvin k$",
];

/// Shapes where JS and the `regex` crate are known to read differently, or
/// where JS is more lenient than people expect.
const ADVERSARIAL_PATTERNS: &[&str] = &[
	"a{,3}",
	"x{2,1}",
	"{",
	"}",
	"{1}",
	"x{1}{",
	"a{1",
	"a{2}{3}",
	"]",
	"[]]",
	"[^]]",
	"[]a]",
	"[]",
	"x[]",
	"[^]",
	"a|",
	"|",
	"()",
	"(?:)*",
	"(|)",
	"a||b",
	"$^",
	"^^a",
	"a$$",
	"^|$",
	"(a|b)*c",
	"(a*)*b",
	"((((((((((a))))))))))",
	"a{0}",
	"a{0,0}b",
	"a{1,}?",
	"a*?b",
	"*",
	"+",
	"?",
	"a**",
	"a+*",
	"a*??",
	"a{1}??",
	"^*",
	"$*",
	"\\b*",
	"\\b\\b",
	"\\bé",
	"é\\b",
	"[\\b]",
	"\\t",
	"\\v",
	"\\f",
	"\\0",
	"\\00",
	"[\\0]",
	"\\8",
	"\\1",
	"(a)\\1",
	"(?:a)\\1",
	"\\k",
	"\\k<a>",
	"(?<a>x)\\k<a>",
	"(?<1a>x)",
	"(?<a>x)|(?<a>y)",
	"(?<a$>x)",
	"(?<é>x)",
	"(?<_>a)",
	"(?i:a)",
	"(?i)a",
	"(?=a)",
	"(?!a)",
	"(?<=a)b",
	"(?<!a)b",
	"(?=a)*",
	"\\p{L}",
	"\\p{2}",
	"\\P{L}",
	"\\u{41}",
	"\\x{41}",
	"\\x4",
	"\\xg1",
	"\\u",
	"\\u12",
	"\\u12g4",
	"\\ud83d",
	"\\cJ",
	"\\c1",
	"[\\c1]",
	"\\c",
	"\\a\\q\\e",
	"\\-",
	"[\\-]",
	"\\_",
	"\\ ",
	"\\/",
	"\\é",
	"[\\d-z]",
	"[a-\\d]",
	"[\\w-]",
	"[-a]",
	"[a-]",
	"[a-b-c]",
	"[a-z--0]",
	"[z-a]",
	"[\\x5a-\\x41]",
	"[\\x41-\\x5a]",
	"[\\u0000-\\uffff]",
	"[[]",
	"[[]]",
	"[a&&b]",
	"[--z]",
	"[\\s\\S]",
	"[\\w\\W]",
	"[^\\s]",
	"[\\u2028]",
	"a\\",
	"\\",
	"(",
	")",
	"[",
	"(?",
	"(?<",
	"(?<a",
	"🍕",
	"[🍕]",
	"a{99999}",
	"a{99999999999}",
	"(?:a{100}){100}",
	"(?:(?:\\s{30}){30}){30}",
];

/// Patterns on id fields aren't lowercased, so the uppercase escapes reach
/// the translator there. The ids are the UUIDs below.
const ID_PATTERNS: &[&str] = &[
	"",
	"^$",
	"^00000004",
	"4000-8000",
	"^[0-9a-f-]{36}$",
	"[A-F]",
	"^\\S+$",
	"\\D",
	"\\W",
	"-\\B",
	"\\B4",
	"\\b4000\\b",
	"^[\\W\\d]",
	"[\\W]",
	"\\X",
	"\\u0034",
	"(?<A>\\d)",
	"0{7}",
	"0{12}$",
	"\\d{3}$",
	"null",
	"(?=0)",
];

const MERCHANTS: &[&str] = &[
	"amzn mktp",
	"costco whse",
	"shell oil",
	"uber\\s+eats",
	"zelle to \\w+",
	"starbucks #\\d+",
	"wal-mart",
	"apple\\.com/bill",
];

/// Rules that list many merchants run long. Up to Veritas's 10,000 character
/// cap they must run and agree with Actual, and past it they must be left to
/// Actual. Each comes with whether Veritas should read it. A run of `\s`
/// expands the most per character (96 bytes each), so 521 of them pass the
/// 50,000 byte translation cap well inside the source cap.
fn sized_patterns() -> Vec<(String, bool)> {
	let alternation = |chars: usize| {
		let mut names: Vec<String> = MERCHANTS.iter().map(|name| (*name).to_owned()).collect();
		while names.join("|").chars().count() < chars {
			names.push(format!("merchant {}", names.len()));
		}
		names.pop();
		names.join("|")
	};
	vec![
		(alternation(10_000), true),
		(alternation(10_200), false),
		("\\s".repeat(520), true),
		("\\s".repeat(521), false),
	]
}

const PAYEES: [&str; 6] = [
	"Amazon",
	"École du ski",
	"ΟΔΟΣ Cafe",
	"Straße Bäckerei",
	"KELVIN \u{212a}",
	"İstanbul Grill",
];

/// Real-looking descriptors plus the content that breaks naive readings:
/// case mappings that change length, line terminators, JS-only whitespace,
/// and astral characters JS splits into surrogates.
const TEXTS: &[&str] = &[
	"",
	"AMZN Mktp US*2K3LM8",
	"Amazon.com",
	"SQ *BLUE BOTTLE",
	"TST* Joe's Pizza",
	"UBER   EATS",
	"PAYPAL *STEAM",
	"Starbucks #12345",
	"POS DEBIT 0420",
	"ACH CREDIT PAYROLL",
	"WAL-MART #3301",
	"Costco Whse",
	"Shell Oil 57442",
	"CHECK 1042",
	"Zelle to Bob",
	"$12.50 tip",
	"APPLE.COM/BILL",
	"me@example.com",
	"PPD ID: 991",
	"Café\u{2028}Bar",
	"École du ski",
	"Straße 9",
	"STRASSE 9",
	"İstanbul Kebap",
	"ΟΔΟΣ ΑΘΗΝΑΣ",
	"KELVIN \u{212a}",
	"line1\r\nline2",
	"line1\nline2",
	"line1\u{85}line2",
	"nbsp\u{a0}here",
	"bom\u{feff}here",
	"x{2}",
	"a]b",
	"aaa",
	"a",
	"-",
	"١٢٣",
	"\u{1c5}\u{301}",
	"tab\there",
	"Pizza 🍕",
	"🍕",
];

const TEXT_ALPHABET: &[char] = &[
	'a', 'b', 'A', 'B', 'z', '0', '1', '9', ' ', '-', '_', '.', 'é', 'É', 'ß', 'İ', 'ı', 'Σ', 'σ',
	'ς', '\u{212a}', 'k', 'ſ', 's', '\n', '\r', '\t', '\u{2028}', '\u{a0}', '\u{feff}', '\u{85}',
	']', '[', '{', '}', '\\', '^', '$', '#', '/', '&', '\u{8}', '\0', '٣', 'ǅ', '\u{301}', 'ﬀ',
	'ẞ',
];

const ESCAPES: &[&str] = &[
	"\\d", "\\D", "\\w", "\\W", "\\s", "\\S", ".", "\\.", "\\-", "\\]", "\\x41", "\\x61",
	"\\u00e9", "\\u00C9", "\\u0130", "\\u212A", "\\u017f", "\\t", "\\n", "\\r", "\\v", "\\f",
	"\\0", "\\cJ", "\\cj", "\\p", "\\q", "\\_", "\\/", "\\ ", "\\é", "\\{", "\\}", "\\$", "\\^",
	"\\x", "\\u", "\\x4", "\\K", "\\Z", "\\e",
];

const CLASS_ESCAPES: &[&str] = &[
	"\\d", "\\w", "\\s", "\\b", "\\-", "\\]", "\\\\", "\\x41", "\\u00e9", "\\n", "\\t", "\\0",
	"\\cJ", "\\D", "\\W", "\\S", "\\B",
];

const CLASS_CHUNKS: &[&str] = &[
	"a-z",
	"A-Z",
	"0-9",
	"\\x00-\\x7f",
	"\\u00c0-\\u00ff",
	"\\x41-\\x5a",
	"-",
	"^",
	"[",
	"&&",
	"--",
	"\\u2028",
];

const QUANTIFIERS: &[&str] = &[
	"*", "+", "?", "*?", "+?", "??", "{2}", "{0}", "{1,}", "{0,2}", "{2,3}?", "{1", "{,2}",
];

fn plain_char(rng: &mut Rng) -> char {
	loop {
		let c = *rng.pick(TEXT_ALPHABET);
		if !"\\[]{}()^$|*+?.".contains(c) {
			return c;
		}
	}
}

fn class_char(rng: &mut Rng) -> char {
	loop {
		let c = *rng.pick(TEXT_ALPHABET);
		if c != '\\' && c != ']' {
			return c;
		}
	}
}

/// A class member list, dashes after class escapes included.
fn class(rng: &mut Rng) -> String {
	let mut out = String::from(if rng.chance(25) { "[^" } else { "[" });
	for _ in 0..rng.below(4) {
		let chunk = match rng.below(6) {
			0 => format!("{}-{}", class_char(rng), class_char(rng)),
			1 => (*rng.pick(CLASS_ESCAPES)).to_owned(),
			2 => (*rng.pick(CLASS_CHUNKS)).to_owned(),
			_ => class_char(rng).to_string(),
		};
		out.push_str(&chunk);
	}
	out.push(']');
	out
}

fn atom(rng: &mut Rng, depth: u32) -> String {
	match rng.below(9) {
		2 => (*rng.pick(ESCAPES)).to_owned(),
		3 => (*rng.pick(&["^", "$", "\\b", "\\B"])).to_owned(),
		4 | 5 => class(rng),
		6 if depth < 4 => {
			let opener = *rng.pick(&["(", "(?:", "(?<g>"]);
			let len = 1 + rng.below(2);
			let mut group = format!("{opener}{}", sequence(rng, len, depth + 1));
			if rng.chance(33) {
				let len = rng.below(2);
				group.push('|');
				group.push_str(&sequence(rng, len, depth + 1));
			}
			group.push(')');
			group
		}
		7 if depth < 4 => format!(
			"{}|{}",
			sequence(rng, 1, depth + 1),
			sequence(rng, 1, depth + 1)
		),
		_ => plain_char(rng).to_string(),
	}
}

fn sequence(rng: &mut Rng, len: u64, depth: u32) -> String {
	let mut out = String::new();
	for _ in 0..len {
		out.push_str(&atom(rng, depth));
		if rng.chance(33) {
			out.push_str(rng.pick(QUANTIFIERS));
		}
	}
	out
}

/// `addTransactions` trims the imported payee with JS's `trim`, which the
/// shared helper approximates with Rust's. They disagree on U+0085 and
/// U+FEFF, so generated texts never start or end with whitespace.
fn generated_text(rng: &mut Rng) -> String {
	let text: String = (0..rng.below(8))
		.map(|_| *rng.pick(TEXT_ALPHABET))
		.collect();
	let edge = |c: Option<char>| c.is_some_and(|c| c.is_whitespace() || c == '\u{feff}');
	if edge(text.chars().next()) || edge(text.chars().last()) {
		format!("z{text}z")
	} else {
		text
	}
}

struct Probe {
	id: RuleId,
	field: &'static str,
	pattern: String,
	readable: bool,
}

fn budget_from(account: &str, payees: &[(String, &str)], dump: &Value) -> BudgetData {
	let mut budget = BudgetData::empty(BudgetType::Envelope);
	budget.accounts.insert(
		AccountId::new(account),
		Account {
			id: AccountId::new(account),
			name: account.to_owned(),
			off_budget: false,
			closed: false,
			group: None,
			sort_order: 0.0,
			tombstone: false,
		},
	);
	for (id, name) in payees {
		budget.payees.insert(
			PayeeId::new(id),
			Payee {
				id: PayeeId::new(id),
				name: (*name).into(),
				transfer_account: None,
				favorite: false,
				learn_categories: true,
				tombstone: false,
			},
		);
	}
	for row in dump["tables"]["rules"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
	{
		let rule = rule_from_row(row);
		budget.rules.insert(rule.id.clone(), rule);
	}
	budget
}

/// The rule numbers whose ` [n]` markers Actual appended to the notes.
fn markers(notes: &Value) -> BTreeSet<usize> {
	notes
		.as_str()
		.unwrap_or_default()
		.split_whitespace()
		.filter_map(|token| token.strip_prefix('[')?.strip_suffix(']')?.parse().ok())
		.collect()
}

#[test]
fn every_matches_verdict_agrees_with_actual_or_is_undetermined() {
	let Some(oracle) = oracle_dir() else {
		eprintln!(
			"skipping the regex oracle test: tools/actual-oracle/node_modules is missing, run `cargo xtask oracle setup`"
		);
		return;
	};
	let mut rng = Rng(0x5eed_1e55_d47d_47ab);
	let account = uuid(1, 1);
	let payees: Vec<(String, &str)> = PAYEES
		.iter()
		.enumerate()
		.map(|(n, name)| (uuid(4, n), *name))
		.collect();
	let mut ops = vec![
		json!({"op": "createAccount", "id": account, "name": "Joint checking", "offBudget": false}),
	];
	for (id, name) in &payees {
		ops.push(json!({"op": "createPayee", "id": id, "name": name}));
	}

	let mut string_patterns: Vec<String> = BANK_PATTERNS
		.iter()
		.chain(ADVERSARIAL_PATTERNS)
		.map(|pattern| (*pattern).to_owned())
		.collect();
	let sized = sized_patterns();
	string_patterns.extend(sized.iter().map(|(pattern, _)| pattern.clone()));
	while string_patterns.len()
		< BANK_PATTERNS.len() + ADVERSARIAL_PATTERNS.len() + sized.len() + GENERATED_PATTERNS
	{
		let len = 1 + rng.below(4);
		string_patterns.push(sequence(&mut rng, len, 0));
	}
	let mut fields: Vec<(&'static str, &'static str, String)> = string_patterns
		.into_iter()
		.map(|pattern| ("imported_payee", "string", pattern))
		.collect();
	for pattern in [
		"^(é|e)cole",
		"^ΟΔΟΣ",
		"ß",
		"strasse",
		"\\u0130",
		"^i\\u0307stanbul",
		"kelvin k$",
	] {
		fields.push(("payee_name", "string", pattern.to_owned()));
	}
	for pattern in ID_PATTERNS {
		fields.push(("payee", "id", (*pattern).to_owned()));
		fields.push(("account", "id", (*pattern).to_owned()));
	}

	let mut probes = Vec::new();
	for (n, (field, kind, pattern)) in fields.into_iter().enumerate() {
		let condition = json!({"op": "matches", "field": field, "value": pattern, "type": kind});
		let id = uuid(8, n);
		let probe_rule = rule_from_row(
			&json!({"id": id, "conditions": json!([condition]).to_string(), "actions": "[]"}),
		);
		let readable = match ParsedRule::parse(&probe_rule).support() {
			RuleSupport::Full => true,
			RuleSupport::ReadOnly(_) => false,
			// Actual's own parse rejects an empty text pattern, so createRule would fail.
			RuleSupport::IgnoredByActual(_) => continue,
		};
		ops.push(
			json!({"op": "createRule", "id": id, "conditions": [condition],
			"actions": [{"op": "append-notes", "value": format!(" [{n}]")}]}),
		);
		probes.push((
			n,
			Probe {
				id: RuleId::new(&id),
				field,
				pattern,
				readable,
			},
		));
	}

	for (pattern, readable) in &sized {
		let probe = probes
			.iter()
			.find(|(_, probe)| probe.pattern == *pattern)
			.unwrap();
		assert_eq!(
			probe.1.readable,
			*readable,
			"{} chars: {}",
			pattern.chars().count(),
			&pattern[..40]
		);
	}

	let mut texts: Vec<String> = TEXTS.iter().map(|text| (*text).to_owned()).collect();
	texts.extend((0..GENERATED_TEXTS).map(|_| generated_text(&mut rng)));
	let transactions: Vec<Value> = texts
		.iter()
		.enumerate()
		.map(|(n, text)| {
			let mut transaction = json!({"id": uuid(9, n), "date": "2026-11-01", "amount": -100,
				"cleared": true, "imported_payee": text});
			if n % 3 != 0 {
				transaction["payee"] = json!(payees[n % payees.len()].0);
			}
			transaction
		})
		.collect();
	ops.push(json!({"op": "addTransactions", "account": account, "transactions": transactions, "runTransfers": false}));

	let scratch = scratch_dir("regex-sweep");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"regex-sweep",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from(&account, &payees, &dump);
	let book = RuleBook::new(&budget);
	let rows: BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect();

	let mut wrong = Vec::new();
	let (mut agreed_hits, mut agreed_misses, mut undetermined) = (0, 0, 0);
	for input in &transactions {
		let text = input["imported_payee"].as_str().unwrap();
		let astral = text.chars().any(|c| c.len_utf16() > 1);
		let outcome = book.run(
			&rule_transaction(input, &account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		let applied: BTreeSet<&RuleId> = outcome.applied.iter().collect();
		let unknown: BTreeSet<&RuleId> = outcome.undetermined.iter().collect();
		let actual = markers(&rows[input["id"].as_str().unwrap()]["notes"]);
		for (n, probe) in &probes {
			let actual_hit = actual.contains(n);
			if unknown.contains(&probe.id) {
				undetermined += 1;
				continue;
			}
			let veritas_hit = applied.contains(&probe.id);
			if !probe.readable {
				wrong.push(format!(
					"{:?} on {}: Veritas refuses the pattern but still gave an answer",
					probe.pattern, probe.field
				));
			} else if probe.field == "imported_payee" && astral {
				wrong.push(format!(
					"{:?} on {text:?}: astral text must be undetermined, Veritas said {veritas_hit}",
					probe.pattern
				));
			} else if veritas_hit != actual_hit {
				wrong.push(format!(
					"{:?} on {} of {text:?}: Actual {actual_hit}, Veritas {veritas_hit}",
					probe.pattern, probe.field
				));
			} else if actual_hit {
				agreed_hits += 1;
			} else {
				agreed_misses += 1;
			}
		}
	}
	let refused = probes.iter().filter(|(_, probe)| !probe.readable).count();
	eprintln!(
		"{} patterns ({refused} refused) × {} texts: {agreed_hits} matches and {agreed_misses} misses agree, {undetermined} undetermined, {} wrong",
		probes.len(),
		transactions.len(),
		wrong.len()
	);
	assert!(
		agreed_hits > 2_000 && agreed_misses > 2_000,
		"the corpus should exercise both answers"
	);
	assert!(
		wrong.is_empty(),
		"{} wrong verdicts:\n{}",
		wrong.len(),
		wrong.join("\n")
	);
}

/// In a JS class, `\d-x` is `\d`, `-` and `x`, and `x` can't start a range
/// (Annex B `ClassAtom - ClassAtom` with a class escape on one side), so
/// `[\d-a-z]` doesn't take in `m` and `[\d--z]` only adds `-` and `z`. Text
/// fields lowercase the pattern, so `\S` and `\W` take the same path.
#[test]
fn js_reads_a_dash_after_a_class_escape_as_a_literal() {
	let Some(oracle) = oracle_dir() else {
		return;
	};
	let account = uuid(1, 1);
	let patterns = [
		"[\\d-a-z]",
		"[\\d--z]",
		"[\\s-_-z]",
		"^[\\w-.-z]+$",
		"[\\S--٣]",
	];
	let texts = ["m", "Zelle", "!", "a.b-c", "٢", "7"];
	let mut ops = vec![
		json!({"op": "createAccount", "id": account, "name": "Joint checking", "offBudget": false}),
	];
	for (n, pattern) in patterns.iter().enumerate() {
		ops.push(json!({"op": "createRule", "id": uuid(8, n),
			"conditions": [{"op": "matches", "field": "imported_payee", "value": pattern, "type": "string"}],
			"actions": [{"op": "append-notes", "value": format!(" [{n}]")}]}));
	}
	let transactions: Vec<Value> = texts
		.iter()
		.enumerate()
		.map(|(n, text)| json!({"id": uuid(9, n), "date": "2026-11-01", "amount": -100, "imported_payee": text}))
		.collect();
	ops.push(json!({"op": "addTransactions", "account": account, "transactions": transactions, "runTransfers": false}));
	let scratch = scratch_dir("regex-class-dash");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"regex-class-dash",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let budget = budget_from(&account, &[], &dump);
	let book = RuleBook::new(&budget);
	let rows: BTreeMap<&str, &Value> = dump["tables"]["transactions"]["rows"]
		.as_array()
		.into_iter()
		.flatten()
		.filter_map(|row| Some((row["id"].as_str()?, row)))
		.collect();
	let mut wrong = Vec::new();
	for input in &transactions {
		let outcome = book.run(
			&rule_transaction(input, &account),
			&budget,
			&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
		);
		let actual = markers(&rows[input["id"].as_str().unwrap()]["notes"]);
		for (n, pattern) in patterns.iter().enumerate() {
			let id = RuleId::new(uuid(8, n));
			let veritas = outcome.applied.contains(&id);
			let undetermined = outcome.undetermined.contains(&id);
			if !undetermined && veritas != actual.contains(&n) {
				wrong.push(format!(
					"{pattern:?} on {}: Actual {}, Veritas {veritas}",
					input["imported_payee"],
					actual.contains(&n)
				));
			}
		}
	}
	assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// `(?-u:\B)` can match between the bytes of one UTF-8 character. The
/// `regex` crate drops that empty match and then misses the alternative that
/// does match, so `\B|é` would fail on "sé1" where JS matches the `é`. Only id
/// fields keep `\B` (text fields lowercase it to `\b`), and real ids are
/// ASCII UUIDs, but Veritas leaves any `\B` on non-ASCII text to Actual.
#[test]
fn a_non_boundary_beside_non_ascii_text_is_left_to_actual() {
	let Some(oracle) = oracle_dir() else {
		return;
	};
	let account = uuid(1, 1);
	let payee = "sé1".to_owned();
	let ops = vec![
		json!({"op": "createAccount", "id": account, "name": "Joint checking", "offBudget": false}),
		json!({"op": "createPayee", "id": payee, "name": "Odd id"}),
		json!({"op": "createRule", "id": uuid(8, 0),
			"conditions": [{"op": "matches", "field": "payee", "value": "\\B|é", "type": "id"}],
			"actions": [{"op": "append-notes", "value": " [0]"}]}),
		json!({"op": "addTransactions", "account": account, "runTransfers": false,
			"transactions": [{"id": uuid(9, 0), "date": "2026-11-01", "amount": -100, "payee": payee}]}),
	];
	let scratch = scratch_dir("regex-non-boundary");
	let dump = apply_and_dump(
		&oracle,
		&scratch,
		"regex-non-boundary",
		&repo_root().join("fixtures/synthetic/small.zip"),
		ops,
		TODAY,
	);
	let _ = std::fs::remove_dir_all(&scratch);
	let row = &dump["tables"]["transactions"]["rows"]
		.as_array()
		.unwrap()
		.iter()
		.find(|row| row["id"] == uuid(9, 0))
		.unwrap()["notes"];
	assert_eq!(markers(row), BTreeSet::from([0]), "Actual matches the é");
	let budget = budget_from(&account, &[(payee.clone(), "Odd id")], &dump);
	let input = json!({"id": uuid(9, 0), "date": "2026-11-01", "amount": -100, "payee": payee});
	let outcome = RuleBook::new(&budget).run(
		&rule_transaction(&input, &account),
		&budget,
		&mut BudgetPayees::new(&budget, || PayeeId::new("made")),
	);
	assert_eq!(outcome.undetermined, [RuleId::new(uuid(8, 0))]);
	assert!(outcome.reasons[&RuleId::new(uuid(8, 0))].contains("non-ASCII"));
}
