//! D73: the formula card, compared with Actual's own `useFormulaExecution`
//! flow, run by the oracle (`widgets`) on the same fixture and meta. These
//! are black-box comparisons of outputs, and the acceptance suite for the
//! clean-room formula engine (D75), so they're ignored until it lands.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod support;

use chrono::NaiveDate;
use serde_json::{Value, json};
use support::actual::{fixture, oracle};
use veritas_domain::{BudgetCell, BudgetData, CategoryId, CivilDate, CoreError, Money, YearMonth};
use veritas_engine::EnvelopeSheet;
use veritas_reports::{
	DEFAULT_FORMULA, FontSizeMode, FormulaCard, FormulaErrorKind, FormulaValue, Json, formula_card,
};

const ORACLE_TODAY: &str = "2026-11-09";

fn today() -> NaiveDate {
	NaiveDate::parse_from_str(ORACLE_TODAY, "%Y-%m-%d").unwrap()
}

fn meta(value: &Value) -> Option<Json> {
	(!value.is_null()).then(|| Json::parse(&value.to_string()).unwrap())
}

fn id_of<'a>(names: impl Iterator<Item = (&'a str, String)>, name: &str) -> String {
	names
		.into_iter()
		.find(|(candidate, _)| *candidate == name)
		.map(|(_, id)| id)
		.unwrap_or_else(|| panic!("no {name}"))
}

fn category(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.categories
			.values()
			.filter(|c| !c.tombstone)
			.map(|c| (c.name.as_str(), c.id.to_string())),
		name,
	)
}

fn group(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.category_groups
			.values()
			.map(|g| (g.name.as_str(), g.id.to_string())),
		name,
	)
}

fn account(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.accounts
			.values()
			.map(|a| (a.name.as_str(), a.id.to_string())),
		name,
	)
}

fn payee(budget: &BudgetData, name: &str) -> String {
	id_of(
		budget
			.payees
			.values()
			.filter(|p| !p.tombstone)
			.map(|p| (p.name.as_str(), p.id.to_string())),
		name,
	)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Expect {
	/// Same value or same error message as Actual.
	Same,
	/// Veritas says it can't evaluate the formula.
	Refused,
	/// A function outside Ryan's scope: Actual computes a value, Veritas
	/// shows the unknown-function error.
	OutOfScope,
}

struct Case {
	meta: Value,
	expect: Expect,
}

fn formula(text: &str, expect: Expect) -> Case {
	Case {
		meta: json!({"formula": text}),
		expect,
	}
}

fn with_queries(text: &str, queries: &Value) -> Case {
	Case {
		meta: json!({"formula": text, "queries": queries}),
		expect: Expect::Same,
	}
}

/// The oracle's number: an integer JSON number, or a non-integer's JS text.
fn oracle_number(value: &Value) -> Option<f64> {
	match value {
		Value::Number(number) => number.as_f64(),
		Value::String(text) => text.parse().ok(),
		_ => None,
	}
}

fn check(
	card: &Result<FormulaCard, CoreError>,
	theirs: &Value,
	expect: Expect,
) -> Result<(), String> {
	let error = theirs["error"].as_str();
	match expect {
		Expect::Refused => match card {
			Err(CoreError::Invalid(reason)) if reason.contains("Veritas can't evaluate") => Ok(()),
			other => Err(format!("expected Veritas to refuse, got {other:?}")),
		},
		Expect::OutOfScope => match card {
			Ok(card)
				if matches!(&card.value, FormulaValue::Error(e)
					if e.kind == FormulaErrorKind::Name && e.message == "Formula error: NAME")
					&& error.is_none() =>
			{
				Ok(())
			}
			other => Err(format!(
				"expected NAME where Actual computes {theirs}, got {other:?}"
			)),
		},
		Expect::Same => {
			let card = card.as_ref().map_err(|e| format!("we failed: {e}"))?;
			let same = match &card.value {
				FormulaValue::Error(ours) => error == Some(ours.message.as_str()),
				_ if error.is_some() => false,
				FormulaValue::Number(ours) => {
					theirs["type"] == "number"
						&& oracle_number(&theirs["value"])
							.is_some_and(|n| n.to_bits() == ours.to_bits())
				}
				FormulaValue::Text(ours) => {
					theirs["type"] == "string" && theirs["value"] == json!(ours)
				}
				FormulaValue::Boolean(ours) => {
					theirs["type"] == "boolean" && theirs["value"] == json!(ours)
				}
				FormulaValue::Empty => theirs["type"] == "null" && theirs["value"].is_null(),
			};
			if same {
				Ok(())
			} else {
				Err(format!("ours {:?}, Actual {theirs}", card.value))
			}
		}
	}
}

/// Runs every case through both apps, after the setup requests.
fn compare(budget: &BudgetData, cases: &[Case]) {
	compare_after(Vec::new(), budget, cases);
}

fn compare_after(setup: Vec<Value>, budget: &BudgetData, cases: &[Case]) {
	let sheet = EnvelopeSheet::compute(budget, CivilDate::from(today())).unwrap();
	let mut requests = setup;
	requests.extend(
		cases
			.iter()
			.enumerate()
			.map(|(index, case)| json!({"id": format!("formula-{index}"), "type": "formula-card", "meta": case.meta})),
	);
	let ours: Vec<_> = cases
		.iter()
		.map(|case| formula_card(budget, &sheet, meta(&case.meta).as_ref(), today()))
		.collect();
	let Some(theirs) = oracle(
		"widgets",
		"all-features",
		&Value::Array(requests),
		ORACLE_TODAY,
	) else {
		return;
	};
	let failures: Vec<String> = cases
		.iter()
		.zip(&ours)
		.enumerate()
		.filter_map(|(index, (case, card))| {
			let theirs = &theirs[format!("formula-{index}").as_str()];
			check(card, theirs, case.expect)
				.err()
				.map(|why| format!("{}: {why}", case.meta))
		})
		.collect();
	assert!(
		failures.is_empty(),
		"{} of {} differ:\n{}",
		failures.len(),
		cases.len(),
		failures.join("\n")
	);
}

const ARITHMETIC: &[&str] = &[
	"=1+2",
	"=1-2",
	"=2*3",
	"=7/2",
	"=2^10",
	"=2^0.5",
	"=2^-2",
	"=-2^2",
	"=2^3^2",
	"=-(2^2)",
	"=+5",
	"=--5",
	"=+-5",
	"=50%",
	"=-50%",
	"=50%*2",
	"=5%^2",
	"=1+2*3",
	"=(1+2)*3",
	"=10/4*2",
	"=10-4-3",
	"=0.1+0.2",
	"=0.1+0.2-0.3",
	"=0.3-0.1-0.2",
	"=1-0.9",
	"=1e3",
	"=1.5e-3",
	"=.5",
	"=5.",
	"=1/3",
	"=2/3",
	"=1e308*10",
	"=1/0",
	"=0/0",
	"=-1/0",
	"=(-8)^(1/3)",
	"=0^0",
	"=0^-1",
	"=1.05^12",
	"=17.21^2",
	"=1E3",
	"=123456789012*1000",
	"=1e21+1",
	"=1/7*7",
	"=100000000000*3.3",
	"=0.1*3",
	"=1e-20",
	"=-0",
	"=3-3",
	"=2.5-2.4999999999999",
	"=1+1e-14",
	"=1e15+0.3",
	"=999999999999999.9",
	"=-1234.5678",
	"=1234.56-0.01",
	"=12020.48/3",
	"=2^1023*2",
];

const COERCION: &[&str] = &[
	"=\"5\"+1",
	"=\"5\"*\"2\"",
	"=\"abc\"+1",
	"=\"\"+1",
	"=\" 5 \"+1",
	"=\"5%\"*2",
	"=\"$5\"+1",
	"=\"5$\"+1",
	"=\"$ 5\"+1",
	"=\"-$5\"+1",
	"=\"$-5\"+1",
	"=\"1e3\"+0",
	"=\"1,000\"+0",
	"=\"+.5\"+0",
	"=\"2026-01-15\"+0",
	"=\"15/01/2026\"+0",
	"=\"01/15/2026\"+0",
	"=\"2026/01/15\"+0",
	"=\"15-01-2026\"+0",
	"=\"2026.01.15\"+0",
	"=\"15/01/\"+0",
	"=\"10:30\"+0",
	"=\"10:30 pm\"+0",
	"=\"10:30:15.5\"+0",
	"=\"2026-01-15 10:30\"+0",
	"=\"13:00 pm\"+0",
	"=\"29/02/2024\"+0",
	"=\"29/02/2023\"+0",
	"=\"15/01/99\"+0",
	"=\"10:\"+0",
	"=\"  2026-01-15  \"+0",
	"=\"5 %\"+0",
	"=TRUE()+1",
	"=-\"abc\"",
	"=\"abc\"%",
	"=+\"abc\"",
	"=TRUE()*FALSE()",
	"=-TRUE()",
];

const COMPARISON: &[&str] = &[
	"=1=1",
	"=1=1.0000000000001",
	"=1=1.000000000001",
	"=1<2",
	"=2<=2",
	"=3>2",
	"=3>=4",
	"=1<>1",
	"=\"abc\"=\"ABC\"",
	"=\"abc\"<\"abd\"",
	"=\"a b\"<\"ab\"",
	"=\"a-b\"<\"ab\"",
	"=\"Z\">\"a\"",
	"=\"10\"<\"9\"",
	"=\"_\"<\"-\"",
	"=\"$\"<\"0\"",
	"=\"~\">\"|\"",
	"=1<\"a\"",
	"=\"a\"<TRUE()",
	"=TRUE()>1",
	"=TRUE()=1",
	"=\"1\"=1",
	"=\"2026-01-15\"=DATE(2026,1,15)",
	"=\"15/01/2026\"=\"2026-01-15\"",
	"=\"\"=0",
	"=IF(1,)=0",
	"=IF(1,)=\"\"",
	"=IF(1,)=FALSE()",
	"=IF(1,)<1",
	"=1/0=1",
	"=1=1/0",
	"=(1=1)=TRUE()",
	"=1=2=FALSE()",
	"=\"é\"=\"é\"",
	"=0=-0",
	"=-1<0",
	"=0.1+0.2=0.3",
];

const LOGIC: &[&str] = &[
	"=IF(TRUE(),1,2)",
	"=IF(FALSE(),1,2)",
	"=IF(1,\"y\")",
	"=IF(0,\"y\")",
	"=IF(\"TRUE\",1,2)",
	"=IF(\"true\",1,2)",
	"=IF(\"abc\",1,2)",
	"=IF(\"\",1,2)",
	"=IF(,1,2)",
	"=IF(1,)",
	"=IF(0,1,)",
	"=IF(1)",
	"=IF(1,2,3,4)",
	"=IF(TRUE(),1,1/0)",
	"=IF(FALSE(),1,1/0)",
	"=IF(1/0,1,2)",
	"=AND(TRUE(),FALSE())",
	"=AND(1,2)",
	"=AND()",
	"=AND(,)",
	"=AND(\"x\")",
	"=AND(1/0,\"x\")",
	"=AND(\"x\",1/0)",
	"=OR(FALSE(),0)",
	"=OR(0,1)",
	"=OR(\"FALSE\",\"TRUE\")",
	"=NOT(0)",
	"=NOT(\"abc\")",
	"=NOT(1,2)",
	"=NOT()",
	"=TRUE()",
	"=FALSE()",
	"=TRUE(1)",
	"=TRUE",
	"=true()",
	"=If(1,2,3)",
];

const AGGREGATES: &[&str] = &[
	"=SUM(1,2,3)",
	"=SUM()",
	"=SUM(1,,2)",
	"=SUM(\"5\",1)",
	"=SUM(\"abc\",1)",
	"=SUM(1/0,1)",
	"=SUM(1,1/0,#N/A)",
	"=SUM(TRUE(),1)",
	"=SUM(0.1,0.2,-0.3)",
	"=SUM(1e308,1e308)",
	"=AVERAGE(1,2,4)",
	"=AVERAGE(1,,2)",
	"=AVERAGE()",
	"=AVERAGE(\"x\")",
	"=AVERAGE(0.1,0.2)",
	"=MIN(3,1,2)",
	"=MIN(,5)",
	"=MAX(-3,-1)",
	"=MAX(\"7\",3)",
	"=MIN(1/0)",
	"=SUM(IF(0,1,),2)",
	"=MAX(1)",
	"=SUM(1, 2)",
];

const ROUNDING: &[&str] = &[
	"=ROUND(2.675,2)",
	"=ROUND(2.5)",
	"=ROUND(-2.5)",
	"=ROUND(1.005,2)",
	"=ROUND(1234.5678,-2)",
	"=ROUND(1.23456,1.5)",
	"=ROUND(-1.25,1)",
	"=ROUND(\"x\")",
	"=ROUND(5,400)",
	"=ROUND(0.1+0.2,15)",
	"=ROUND(1/3,10)",
	"=ROUND()",
	"=ABS(-3)",
	"=ABS(\"-3\")",
	"=ABS()",
	"=ABS(1,2)",
	"=ABS(-0.5)",
];

const TEXT: &[&str] = &[
	"=CONCATENATE(\"a\",1,TRUE())",
	"=CONCATENATE()",
	"=CONCATENATE(1/3)",
	"=CONCATENATE(\"a\",1/0)",
	"=\"a\"&\"b\"",
	"=1&2",
	"=1/3&\"\"",
	"=1e21&\"\"",
	"=0.000001&\"\"",
	"=1e-7&\"\"",
	"=TRUE()&FALSE()",
	"=IF(1,)&\"x\"",
	"=-0.5&\"\"",
	"=2^70&\"\"",
	"=LEN(\"hello\")",
	"=LEN(123.5)",
	"=LEN(\"\")",
	"=LEN(\"é😀\")",
	"=LEFT(\"hello\",2)",
	"=LEFT(\"hello\")",
	"=LEFT(\"hello\",0)",
	"=LEFT(\"hello\",-1)",
	"=LEFT(\"hello\",2.7)",
	"=LEFT(\"hello\",99)",
	"=RIGHT(\"hello\",2)",
	"=RIGHT(\"hello\")",
	"=RIGHT(\"hello\",0)",
	"=RIGHT(\"hello\",0.5)",
	"=RIGHT(\"hello\",-1)",
	"=RIGHT(\"hello\",99)",
	"=RIGHT(12345,2)",
	"=UPPER(\"Straße\")",
	"=LOWER(\"ÀB\")",
	"=UPPER(\"abc\")",
	"=LOWER(TRUE())",
	"=\"a\\\"b\"",
	"=\"a\"\"b\"",
];

const DATES: &[&str] = &[
	"=TODAY()",
	"=TODAY()+1",
	"=DATE(2026,1,31)",
	"=DATE(2026,13,1)",
	"=DATE(2026,0,1)",
	"=DATE(2026,-1,1)",
	"=DATE(2024,2,29)",
	"=DATE(2023,2,29)",
	"=DATE(26,1,1)",
	"=DATE(1899,12,31)",
	"=DATE(1500,1,1)",
	"=DATE(10000,1,1)",
	"=DATE(2026.9,1.9,1.9)",
	"=DATE(2026,1,0)",
	"=DATE(2026,1,-1)",
	"=DATE(2000,2,30)",
	"=DATE(1900,2,29)",
	"=YEAR(46037)",
	"=MONTH(46037)",
	"=DAY(46037)",
	"=YEAR(-1)",
	"=YEAR(0)",
	"=MONTH(0)",
	"=DAY(0)",
	"=DAY(60)",
	"=DAY(61)",
	"=YEAR(\"2026-01-15\")",
	"=MONTH(\"15/01/2026\")",
	"=DAY(DATE(2024,2,29))",
	"=YEAR(TODAY())",
	"=MONTH(TODAY())",
	"=DAY(TODAY())",
	"=EOMONTH(DATE(2026,1,15),0)",
	"=EOMONTH(DATE(2026,1,31),1)",
	"=EOMONTH(DATE(2024,1,31),1)",
	"=EOMONTH(DATE(2023,1,31),1)",
	"=EOMONTH(DATE(2026,1,15),-1)",
	"=EOMONTH(DATE(2026,1,15),-13)",
	"=EOMONTH(DATE(2026,1,15),1.5)",
	"=EOMONTH(-1,0)",
	"=EOMONTH(1,-1)",
	"=EOMONTH(TODAY(),0)",
	"=EOMONTH(DATE(9999,12,1),1)",
	"=EOMONTH(DATE(2100,2,1),0)",
	"=YEAR(1e15)",
	"=DAY(45.9)",
	"=YEAR(DATE(2026,1,1)-1)",
];

const ERRORS: &[&str] = &[
	"=#DIV/0!",
	"=#N/A",
	"=#NAME?",
	"=#VALUE!",
	"=#REF!",
	"=#NUM!",
	"=#ERROR!",
	"=#CYCLE!",
	"=#FOO!",
	"=foo",
	"=FOO(1)",
	"=FOO(1/0)",
	"=1+",
	"=",
	"=(1",
	"=1)",
	"=SUM(1 2)",
	"=SUM (1)",
	"=1 2",
	"1+1",
	"SUM(1)",
	" =1",
	"=  1  +  2  ",
	"=\t1",
	"=1;2",
	"=1,2",
	"=RC",
	"=_x",
	"=x.y",
	"=1E3",
	"=1:",
	"=\u{a0}1",
	"=Sheet1!",
	"=$",
	"=1+ +1",
];

const REFUSED: &[&str] = &[
	"={1,2}",
	"=A1",
	"=A1:B2",
	"=ZZZZ1",
	"=OFFSET(A1,0,0)",
	"=QUERY_EXTRACT_CATEGORIES(\"all\")",
	"=SUM({1,2})",
	"=\"é\"=\"e\"",
	"=\"résumé\"<\"resume\"",
	"=A:B+1",
];

const OUT_OF_SCOPE: &[&str] = &[
	"=IFERROR(1/0,2)",
	"=SQRT(4)",
	"=POWER(2,3)",
	"=MID(\"hello\",2,2)",
	"=TEXT(1,\"0.00\")",
	"=VALUE(\"5\")",
	"=COUNT(1,2)",
	"=INTEGER_TO_AMOUNT(12345)",
	"=FIXED(1.234,1)",
	"=QUERY_COUNT(\"all\")",
	"=BALANCE_OF(\"Checking\")",
	"=FORMATNUMBER(1234.5)",
	"=FORMATCURRENCY(12)",
	"=EDATE(DATE(2026,1,31),1)",
	"=INT(2.5)",
	"=IFERROR(QUERY(\"all\"),0)",
];

#[test]
#[ignore = "clean-room formula engine pending"]
fn d73_formula_card_matches_hyperformula() {
	let budget = fixture("all-features");
	let mut cases: Vec<Case> = [
		ARITHMETIC, COERCION, COMPARISON, LOGIC, AGGREGATES, ROUNDING, TEXT, DATES, ERRORS,
	]
	.concat()
	.into_iter()
	.map(|text| formula(text, Expect::Same))
	.collect();
	cases.extend(REFUSED.iter().map(|text| formula(text, Expect::Refused)));
	cases.extend(
		OUT_OF_SCOPE
			.iter()
			.map(|text| formula(text, Expect::OutOfScope)),
	);
	for meta in [
		json!(null),
		json!({}),
		json!({"formula": ""}),
		json!({"formula": null}),
	] {
		cases.push(Case {
			meta,
			expect: Expect::Same,
		});
	}
	compare(&budget, &cases);
}

fn queries(budget: &BudgetData) -> Value {
	let groceries = category(budget, "Groceries");
	let dining = category(budget, "Dining Out");
	let rent = category(budget, "Rent");
	let everyday = group(budget, "Everyday");
	let income = group(budget, "Income");
	let frame = |mode: &str| json!({"mode": mode});
	let range =
		|start: &str, end: &str, mode: &str| json!({"start": start, "end": end, "mode": mode});
	let only = |frame: Value| json!({"timeFrame": frame});
	let cond = |field: &str, op: &str, value: Value| json!({"field": field, "op": op, "value": value, "type": "id"});
	let static_frame = json!({"start": "2026-01", "end": "2026-06", "mode": "static"});
	json!({
		"all": {},
		"static": only(range("2026-01", "2026-03", "static")),
		"days": only(range("2026-01-10", "2026-02-20", "static")),
		"sliding": only(range("2026-05", "2026-07", "sliding-window")),
		"slidingDays": only(range("2026-05-03", "2026-07-20", "sliding-window")),
		"full": only(range("2025-06", "2025-07", "full")),
		"lastMonth": only(frame("lastMonth")),
		"lastYear": only(frame("lastYear")),
		"yearToDate": only(frame("yearToDate")),
		"priorYearToDate": only(frame("priorYearToDate")),
		"currentQuarter": only(frame("currentQuarter")),
		"previousQuarter": only(frame("previousQuarter")),
		"noMode": {"timeFrame": {"start": "2026-01", "end": "2026-02"}},
		"bogusMode": only(range("2026-01", "2026-02", "bogus")),
		"rolledMonth": only(range("2026-13", "2027-02", "static")),
		"groceries": {"conditions": [cond("category", "is", json!(groceries))], "timeFrame": static_frame},
		"diningOrRent": {"conditions": [cond("category", "is", json!(dining)), cond("category", "is", json!(rent))], "conditionsOp": "or", "timeFrame": static_frame},
		"everyday": {"conditions": [cond("category_group", "is", json!(everyday))], "timeFrame": range("2026-06", "2026-11", "static")},
		"notIncome": {"conditions": [cond("category_group", "isNot", json!(income))], "timeFrame": frame("yearToDate")},
		"contains": {"conditions": [cond("category", "contains", json!("o"))], "timeFrame": static_frame},
		"matches": {"conditions": [cond("category", "matches", json!("^(gro|din)"))], "timeFrame": static_frame},
		"oneOf": {"conditions": [cond("category", "oneOf", json!([groceries, rent]))], "timeFrame": static_frame},
		"groupContains": {"conditions": [cond("category_group", "contains", json!("bill"))], "timeFrame": frame("currentQuarter")},
		"andBoth": {"conditions": [cond("category", "contains", json!("o")), cond("category_group", "is", json!(everyday))], "timeFrame": static_frame},
		"amountLt": {"conditions": [{"field": "amount", "op": "lt", "value": 0, "type": "number"}], "timeFrame": static_frame},
		"payee": {"conditions": [cond("payee", "is", json!(payee(budget, "Landlord")))]},
		"account": {"conditions": [cond("account", "is", json!(account(budget, "Checking")))], "timeFrame": frame("lastMonth")},
		"accountOr": {"conditions": [cond("account", "is", json!(account(budget, "Checking"))), {"field": "amount", "op": "gt", "value": 100_000, "type": "number"}], "conditionsOp": "or"},
		"notes": {"conditions": [{"field": "notes", "op": "contains", "value": "a", "type": "string"}]},
		"custom": {"conditions": [{"field": "category", "op": "is", "value": groceries, "type": "id", "customName": "Saved"}], "timeFrame": static_frame},
		"nullConfig": null,
		"badConditions": {"conditions": "x"},
	})
}

#[test]
#[ignore = "clean-room formula engine pending"]
fn d73_formula_card_queries_match_actual() {
	let budget = fixture("all-features");
	let queries = queries(&budget);
	let names: Vec<String> = queries.as_object().unwrap().keys().cloned().collect();
	let mut cases = Vec::new();
	for name in names.iter().map(String::as_str).chain(["missing"]) {
		cases.push(with_queries(&format!("=QUERY(\"{name}\")"), &queries));
		cases.push(with_queries(
			&format!(
				"=QUERY_EXTRACT_TIMEFRAME_START(\"{name}\")&\"|\"&QUERY_EXTRACT_TIMEFRAME_END(\"{name}\")"
			),
			&queries,
		));
	}
	for text in [
		"=QUERY(\"all\")-QUERY(\"static\")",
		"=ROUND(QUERY(\"all\")/3,2)",
		"=QUERY(QUERY_EXTRACT_TIMEFRAME_START(\"static\"))",
		"=IF(QUERY(\"static\")<0,\"spent\",\"earned\")",
		"=QUERY(\"static\")&\"\"",
		"=QUERY()",
		"=QUERY(\"all\",\"static\")",
		"=SUM(1/0,QUERY(\"all\"))",
		"=QUERY(\"STATIC\")",
	] {
		cases.push(with_queries(text, &queries));
	}
	cases.push(Case {
		meta: json!({"formula": "=QUERY(\"constructor\")", "queries": queries}),
		expect: Expect::Refused,
	});
	compare(&budget, &cases);
}

#[test]
#[ignore = "clean-room formula engine pending"]
fn d73_formula_card_budget_queries_match_actual() {
	let budget = fixture("all-features");
	let queries = queries(&budget);
	let groceries = category(&budget, "Groceries");
	let rent = category(&budget, "Rent");
	let dining = category(&budget, "Dining Out");
	let vacation = category(&budget, "Vacation");
	let income = category(&budget, "Income");
	let everyday = group(&budget, "Everyday");
	let mut cases = Vec::new();
	let dimensions = ["budgeted", "spent", "balance_start", "balance_end", "goal"];
	for dimension in dimensions {
		for name in [
			"all",
			"static",
			"groceries",
			"everyday",
			"diningOrRent",
			"contains",
			"matches",
			"oneOf",
			"custom",
			"missing",
			"lastYear",
			"currentQuarter",
			"full",
			"notIncome",
			"groupContains",
			"andBoth",
			"sliding",
			"nullConfig",
		] {
			cases.push(with_queries(
				&format!(
					"=BUDGET_QUERY(\"{dimension}\", QUERY_EXTRACT_CATEGORIES(\"{name}\"), QUERY_EXTRACT_TIMEFRAME_START(\"{name}\"), QUERY_EXTRACT_TIMEFRAME_END(\"{name}\"))"
				),
				&queries,
			));
		}
		for (start, end) in [
			("2026-01", "2026-03"),
			("2025-08", "2025-10"),
			("2025-10", "2025-12"),
			("2026-11", "2027-02"),
			("2027-06", "2027-11"),
			("1990-01", "1990-02"),
			("2030-01", "2030-01"),
			("2026-03", "2026-01"),
			("2026-01-15", "2026-02-01"),
			("2026-13", "2027-01"),
			("abc", "2026-01"),
			("2026", "2026-02"),
			("", "2026-02"),
		] {
			cases.push(with_queries(
				&format!(
					"=BUDGET_QUERY(\"{dimension}\", QUERY_EXTRACT_CATEGORIES(\"all\"), \"{start}\", \"{end}\")"
				),
				&queries,
			));
		}
		for categories in [
			format!("\"{groceries}\""),
			format!("\"{groceries}, {rent},,\""),
			format!("{{\"{dining}\";\"{vacation}\"}}"),
			format!("{{\"{dining}\",\"{vacation}\"}}"),
			format!("\"{everyday}\""),
			format!("\"{income}\""),
			"\"\"".to_owned(),
			"5".to_owned(),
			"TRUE()".to_owned(),
			"1/0".to_owned(),
			"(QUERY_EXTRACT_CATEGORIES(\"groceries\"))".to_owned(),
		] {
			cases.push(with_queries(
				&format!("=BUDGET_QUERY(\"{dimension}\", {categories}, \"2026-09\", \"2026-11\")"),
				&queries,
			));
		}
	}
	for text in [
		"=BUDGET_QUERY(\"SPENT\", QUERY_EXTRACT_CATEGORIES(\"all\"), \"2026-01\", \"2026-03\")",
		"=BUDGET_QUERY(\"bogus\", QUERY_EXTRACT_CATEGORIES(\"all\"), \"2026-01\", \"2026-03\")",
		"=BUDGET_QUERY(1, QUERY_EXTRACT_CATEGORIES(\"all\"), \"2026-01\", \"2026-03\")",
		"=BUDGET_QUERY(\"spent\")",
		"=BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(\"all\"), \"2026-01\")",
		"=BUDGET_QUERY(\"budgeted\", QUERY_EXTRACT_CATEGORIES(\"all\"), \"2026-01\", \"2026-03\") - BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(\"all\"), \"2026-01\", \"2026-03\")",
		"=BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(\"all\"), QUERY_EXTRACT_TIMEFRAME_START(\"static\"), \"2026-06\")+QUERY(\"static\")",
		"=BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(1/0), \"2026-01\", \"2026-03\")",
		"=BUDGET_QUERY(1/0, QUERY_EXTRACT_CATEGORIES(\"all\"), \"bad\", \"2026-03\")",
	] {
		cases.push(with_queries(text, &queries));
	}
	compare(&budget, &cases);
}

/// Budget rows in months the sheet doesn't cover, which Actual still loads
/// into budget cells (`server/sheet.ts:241-255`).
#[test]
#[ignore = "clean-room formula engine pending"]
fn d73_formula_card_reads_budget_rows_outside_the_sheet() {
	let mut budget = fixture("all-features");
	let groceries = category(&budget, "Groceries");
	let rent = category(&budget, "Rent");
	let mut setup = Vec::new();
	for (month, id, amount) in [("2029-03", &groceries, 5_000), ("2024-01", &rent, 7_000)] {
		let key = (
			YearMonth::parse(month).unwrap(),
			CategoryId::new(id.as_str()),
		);
		budget.budget_cells.insert(
			key.clone(),
			BudgetCell {
				month: key.0,
				category: key.1,
				budgeted: Money::from_minor(amount),
				carryover: false,
				goal: None,
				long_goal: false,
			},
		);
		setup.push(json!({"id": format!("edit-{month}"), "type": "budget-amount", "meta": {"month": month, "category": id, "amount": amount}}));
	}
	let mut cases = Vec::new();
	for dimension in ["budgeted", "spent", "balance_start", "balance_end", "goal"] {
		for (categories, start, end) in [
			(groceries.as_str(), "2029-01", "2029-06"),
			(groceries.as_str(), "2029-04", "2029-05"),
			(rent.as_str(), "2023-12", "2024-02"),
			(rent.as_str(), "2024-02", "2024-03"),
		] {
			cases.push(formula(
				&format!(
					"=BUDGET_QUERY(\"{dimension}\", \"{categories}\", \"{start}\", \"{end}\")"
				),
				Expect::Same,
			));
		}
	}
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::from(today())).unwrap();
	let first = formula_card(&budget, &sheet, meta(&cases[0].meta).as_ref(), today()).unwrap();
	assert_eq!(first.value, FormulaValue::Number(50.0));
	compare_after(setup, &budget, &cases);
}

/// The card's other settings, which Actual reads straight from the meta.
#[test]
fn formula_card_reads_its_display_settings() {
	let budget = fixture("all-features");
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::from(today())).unwrap();
	let card =
		|value: Value| formula_card(&budget, &sheet, meta(&value).as_ref(), today()).unwrap();
	let plain = card(json!(null));
	assert_eq!(plain.formula, DEFAULT_FORMULA);
	assert!(plain.show_title);
	assert_eq!(plain.font_size_mode, FontSizeMode::Dynamic);
	assert_eq!(plain.static_font_size, 32.0);
	let styled = card(
		json!({"formula": "=1", "showTitle": false, "fontSizeMode": "static", "staticFontSize": 48}),
	);
	assert!(!styled.show_title);
	assert_eq!(styled.font_size_mode, FontSizeMode::Static);
	assert_eq!(styled.static_font_size, 48.0);
	assert_eq!(card(json!({"staticFontSize": 0})).static_font_size, 32.0);
	assert!(card(json!({"showTitle": null})).show_title);
}

/// The default formula's value, and a result Rust strings can't hold.
#[test]
#[ignore = "clean-room formula engine pending"]
fn formula_card_evaluates_its_default_and_refuses_half_a_character() {
	let budget = fixture("all-features");
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::from(today())).unwrap();
	let plain = formula_card(&budget, &sheet, None, today()).unwrap();
	assert_eq!(plain.value, FormulaValue::Number(6.0));
	// Actual's LEFT returns half of the emoji, which Rust strings can't hold.
	assert!(
		formula_card(
			&budget,
			&sheet,
			meta(&json!({"formula": "=LEFT(\"😀x\",1)"})).as_ref(),
			today()
		)
		.is_err()
	);
}
