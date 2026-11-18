//! D73: the formula card, compared with Actual's own `useFormulaExecution`
//! flow, run by the oracle (`widgets`) on the same fixture and meta. These
//! are black-box comparisons of outputs, and the acceptance suite for the
//! clean-room formula engine (D75).

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
	"=LOG10(100)",
];

/// Tokens, precedence, whitespace and parse errors.
const GRAMMAR_EDGES: &[&str] = &[
	"=R1C1",
	"=rc",
	"=R",
	"=C",
	"=R1C",
	"=RR",
	"=CR",
	"=TRUE()+FALSE",
	"=_",
	"=x_1",
	"=.x",
	"=a.b.c",
	"=\"a\\\\\"&\"z\"",
	"=\"a\\\"",
	"=\"a\nb\"",
	"=\"\"",
	"=\"a\"\"\"",
	"=#n/a",
	"=#N/A2",
	"=#SPILL!",
	"=#LIC!",
	"=#div/0!",
	"=#NAME",
	"=#N/A+1",
	"=1+#N/A",
	"=SUM(#N/A)",
	"=1e400",
	"=1e",
	"=1e+3",
	"=1.2.3",
	"=..5",
	"=1..2",
	"=01",
	"=0x10",
	"=1 +2",
	"= 1",
	"=1\r\n+2",
	"=SUM(1, ,2)",
	"=SUM( 1 )",
	"=SUM(1,)",
	"=SUM(,)",
	"=SUM(,1)",
	"=(1)(2)",
	"=((1))",
	"=-(-(1))",
	"=1--1",
	"=1+-+-1",
	"=2*-3",
	"=2^-3^2",
	"=-2^-2",
	"=50%%",
	"=(50)%",
	"=-50%^2",
	"=2^50%",
	"=1&2+3",
	"=1+2&3",
	"=1=1&\"\"",
	"=1<2=TRUE()",
	"=1&2=12",
	"=\"12\"=1&2",
	"=1<>2<>3",
	"=1>=1>=1",
	"=1=<2",
	"=1=>2",
	"=1><2",
	"=1<=>2",
	"=1==",
	"=1==1",
	"=TRUE()+TRUE()",
	"=TRUE()&1",
	"=SUM(1;2)",
	"=sum(1,2)",
	"=Sum(1,2)",
	"=SUM(1,2)+",
	"=SUM(1,2",
	"=SUM)",
	"=()",
	"=(,)",
	"=IF(1,2,)+1",
	"=IF(,)",
	"=IF(,,)",
	"=+",
	"=-",
	"=%",
	"=1%",
	"=1 %",
	"=1% ",
	"=(1)%",
	"=TODAY() ",
	"= TODAY()",
	"=TODAY( )",
	"=TODAY(1)",
	"=TODAY(,)",
	"=SUM(1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,18,19,20,21,22,23,24,25,26,27,28,29,30,31)",
	"=1E+3",
	"=2e3e",
	"=1e3a",
	"=1a",
	"=1 a",
	"=a 1",
	"=@1",
	"=!1",
	"=~1",
	"=1 ' 2",
	"='a'",
	"=[1]",
	"=1#",
	"=#",
	"=\u{a0}",
	"=1\u{a0}",
	"=\u{3000} 1",
	"=#N/A/1",
	"=#N/A1",
	"=#NUM!1",
	"=#N/A_",
	"=#N/A.5",
	"=#DIV/0!/1",
	"=#REF!A1",
	"=#N/A?",
	"=#VALUE!!",
	"=#N/A!",
	"=#NAME??",
	"=#N/A/1e3%",
];

/// Text read as a number: signs, `$`, `%`, dates and times.
const TEXT_AS_NUMBER_EDGES: &[&str] = &[
	"=\"5\"+0",
	"=\"-5\"+0",
	"=\"+5\"+0",
	"=\"--5\"+0",
	"=\" -5\"+0",
	"=\"- 5\"+0",
	"=\"5 \"+0",
	"=\"\t5\"+0",
	"=\"\n5\"+0",
	"=\"\u{a0}5\"+0",
	"=\"5e\"+0",
	"=\"5E3\"+0",
	"=\"5e+3\"+0",
	"=\".\"+0",
	"=\"5.\"+0",
	"=\"1.5\"+0",
	"=\"1,5\"+0",
	"=\"0x10\"+0",
	"=\"Infinity\"+0",
	"=\"NaN\"+0",
	"=\"1e400\"+0",
	"=\"5%%\"+0",
	"=\"%5\"+0",
	"=\"%\"+0",
	"=\"$\"+0",
	"=\"$5%\"+0",
	"=\"5%$\"+0",
	"=\"$$5\"+0",
	"=\"5 $\"+0",
	"=\"-5$\"+0",
	"=\"5-$\"+0",
	"=\"$5.5\"+0",
	"=\"€5\"+0",
	"=\"-5%\"+0",
	"=\"TRUE\"+0",
	"=\"1/2\"+0",
	"=\"1/2/2026\"+0",
	"=\"2026-1-5\"+0",
	"=\"2026-01-5\"+0",
	"=\"5/1/2026\"+0",
	"=\"05/01/26\"+0",
	"=\"05/01/30\"+0",
	"=\"05/01/31\"+0",
	"=\"05/01/2\"+0",
	"=\"05/01/202\"+0",
	"=\"05/01/02026\"+0",
	"=\"15/01\"+0",
	"=\"01/15\"+0",
	"=\"15/13\"+0",
	"=\"15/1/\"+0",
	"=\"/01/2026\"+0",
	"=\"15//2026\"+0",
	"=\"2026-01\"+0",
	"=\"2026-13\"+0",
	"=\"2026/01\"+0",
	"=\"01/2026\"+0",
	"=\"2026-01-15-\"+0",
	"=\"2026 01 15\"+0",
	"=\"15 01 2026\"+0",
	"=\"15  01 2026\"+0",
	"=\"15/01-2026\"+0",
	"=\"2026-01-15T10:00\"+0",
	"=\"2026-01-15  10:30\"+0",
	"=\"10:30 2026-01-15\"+0",
	"=\"0:0\"+0",
	"=\"24:00\"+0",
	"=\"25:00\"+0",
	"=\"10:60\"+0",
	"=\"10:59:60\"+0",
	"=\"10:59:59.999\"+0",
	"=\"100:00\"+0",
	"=\"10:30am\"+0",
	"=\"10:30AM\"+0",
	"=\"10:30 a\"+0",
	"=\"12:30 am\"+0",
	"=\"12:30 pm\"+0",
	"=\"0:30 pm\"+0",
	"=\"0:30 am\"+0",
	"=\"10:30 p.m.\"+0",
	"=\"10\"+0",
	"=\"10 pm\"+0",
	"=\":30\"+0",
	"=\"10:30:\"+0",
	"=\"10::30\"+0",
	"=\"10:30:15:20\"+0",
	"=\"1:2:3\"+0",
	"=\"10:30.5\"+0",
	"=\"10.5:30\"+0",
	"=\"-10:30\"+0",
	"=\"2026-01-15 25:00\"+0",
	"=\"2026-02-30\"+0",
	"=\"2026-00-10\"+0",
	"=\"2026-01-00\"+0",
	"=\"0000-01-01\"+0",
	"=\"0001-01-01\"+0",
	"=\"1899-12-30\"+0",
	"=\"1899-12-29\"+0",
	"=\"1900-02-29\"+0",
	"=\"1900-03-01\"+0",
	"=\"9999-12-31\"+0",
	"=\"10000-01-01\"+0",
	"=\"1500-01-01\"+0",
	"=\"0099-01-01\"+0",
	"=\"99-01-01\"+0",
	"=\"15/01/0099\"+0",
	"=\"15/01/1899\"+0",
	"=\"jan 15 2026\"+0",
	"=\"15-Jan-2026\"+0",
	"=\"2026-01-15 10:30 pm\"+0",
	"=\"2026-01-15 10\"+0",
	"=\"2026-01-15 abc\"+0",
	"=\"2026-01-1510:30\"+0",
	"=\"5 5\"+0",
	"=\"1 000\"+0",
	"=\"15/01/0\"+0",
	"=\"15/01/00\"+0",
	"=\"15/01/000\"+0",
	"=\"15/01/1\"+0",
	"=\"15/01/ \"+0",
	"=\"15/01/2000\"+0",
	"=\"15/01/  \"+0",
	"=\"15/01/ 10:30\"+0",
	"=\"15/01/.\"+0",
	"=\"15/01//\"+0",
	"=\"2026-01-\"+0",
	"=\"2026/01/\"+0",
	"=\"-01-15\"+0",
	"=\"/01/15\"+0",
	"=\"15/01/1582\"+0",
	"=\"15/01/1583\"+0",
	"=\"15/01/1600\"+0",
	"=\"15/01/1700\"+0",
	"=\"15/01/1800\"+0",
	"=\"15/01/1850\"+0",
	"=\"15/01/1898\"+0",
	"=\"15/01/1000\"+0",
	"=\"15/01/999\"+0",
	"=\"15/01/1001\"+0",
	"=\"15/01/1100\"+0",
	"=\"15/01/1200\"+0",
	"=\"15/01/1300\"+0",
	"=\"15/01/1400\"+0",
	"=\"15/01/1499\"+0",
	"=\"15/01/1501\"+0",
	"=\"15/01/1550\"+0",
	"=\"31/12/1499\"+0",
	"=\"01/01/1500\"+0",
	"=\"10/11/12\"+0",
	"=\"1/1/1\"+0",
	"=\"001/01/2026\"+0",
	"=\"15/001/2026\"+0",
	"=\"0015/01/2026\"+0",
	"=\"2026-001-15\"+0",
	"=\"+15/01/2026\"+0",
	"=\"15/+1/2026\"+0",
	"=\"1.5/01/2026\"+0",
	"=\"15/01/2026.5\"+0",
	"=\"15.01.2026\"+0",
	"=\"15. 01. 2026\"+0",
	"=\"15 / 01 / 2026\"+0",
	"=\"2026-01-15 \"+0",
	"=\"15/01/2026 10:30:15.25 PM\"+0",
	"=\"15/01/2026 13:00 am\"+0",
	"=\"15/01/2026 00:00\"+0",
	"=\"15/01/2026 24:00\"+0",
	"=\"15/01/2026 10:30 xx\"+0",
	"=\"15/01/2026 10:30 pmx\"+0",
	"=\"15/01/2026 10:30 p\"+0",
	"=\"15/01/2026 10:30 P\"+0",
	"=\"10:30 P\"+0",
	"=\"10:30 A M\"+0",
	"=\"10:30  pm\"+0",
	"=\"10: 30\"+0",
	"=\"10 :30\"+0",
	"=\"1e1:30\"+0",
	"=\"10:3e1\"+0",
	"=\"10:30:1.5e1\"+0",
	"=\"10:30:15.\"+0",
	"=\"10:30:.5\"+0",
	"=\"+10:30\"+0",
	"=\"10:+30\"+0",
	"=\"57%\"+0",
	"=57%",
	"=57%&\"\"",
	"=(\"57%\"+0)&\"\"",
	"=(\"0.57\"+0)&\"\"",
	"=\"57 %\"*1",
	"=\"1e-400\"+0",
	"=\"-0\"+0",
	"=\"00005\"+0",
	"=\"5.5.5\"+0",
	"=\"$5$\"+0",
	"=\"$ -5\"+0",
	"=\"-5 $\"+0",
	"=\"- $5\"+0",
	"=\"$+5\"+0",
	"=\"$1e3\"+0",
	"=\"1e3%\"+0",
	"=\"$.5\"+0",
	"=\"$ \"+0",
	"=\"5%  \"+0",
	"=\"  5  %  \"+0",
	"=\"15 01 2026  10:30\"+0",
	"=\"15 01 2026\t10:30\"+0",
	"=\"15/01/2026  10:30\"+0",
	"=\"15 01  2026 10:30\"+0",
	"=\"15  01 2026 10:30\"+0",
	"=\"15 01 2026 10:30  pm\"+0",
	"=\"15/01/2026  10:30  pm\"+0",
	"=\"15-01 2026  10:30\"+0",
	"=\"15  01  2026\"+0",
	"=\"15   01 2026\"+0",
	"=\"10:-0\"+0",
	"=\"10:-5\"+0",
	"=\"-0:30\"+0",
	"=\":-0\"+0",
	"=\"10:30:-0\"+0",
	"=\"10:-0 pm\"+0",
	"=\"10:-0.0\"+0",
	"=\"10:00\u{a0}pm\"+0",
	"=-\":-0P\"",
];

/// Comparisons, empty values, IF, AND, OR, NOT and the aggregates.
const LOGIC_EDGES: &[&str] = &[
	"=\"15/01/1581\"+0",
	"=\"31/12/1581\"+0",
	"=\"01/01/1582\"+0",
	"=\"14/10/1582\"+0",
	"=\"15/10/1582\"+0",
	"=\":\"+0",
	"=\"::\"+0",
	"=\"10:99\"+0",
	"=\"10:30:99\"+0",
	"=\"10:30:.\"+0",
	"=\"10:30:15.5.5\"+0",
	"=\"0:0 am\"+0",
	"=\"12:00 am\"+0",
	"=\"12:00\"+0",
	"=\"15 01 2026 10:30\"+0",
	"=\"15/01 10:30\"+0",
	"=\"\u{a0}15/01/2026\u{a0}\"+0",
	"=\"15/01/2026\u{a0}10:30\"+0",
	"=\"15/01/2026\t10:30\"+0",
	"=\"15\u{a0}01\u{a0}2026\"+0",
	"=\"5%\"=0.05",
	"=\"$5\"=5",
	"=\"10:30\"=0.4375",
	"=\"2026-01-15\"<46038",
	"=\"2026-01-15\">\"a\"",
	"=\"15/01/2026\"<\"2026-01-14\"",
	"=\"5\"<6",
	"=\"5\"<\"6\"",
	"=\"1e3\"=\"1000\"",
	"=\"\"<\"a\"",
	"=\"\"=\"\"",
	"=IF(1,)=IF(1,)",
	"=IF(1,)<\"a\"",
	"=IF(1,)<\"\"",
	"=IF(1,)>-1",
	"=IF(1,)<TRUE()",
	"=FALSE()<TRUE()",
	"=FALSE()=0",
	"=\"TRUE\"=TRUE()",
	"=1<1.0000000000001",
	"=1.0000000000001>1",
	"=1<=1.0000000000001",
	"=1<>1.0000000000001",
	"=0=1e-300",
	"=-0.0000000000001=0.0000000000001",
	"=1e-300=2e-300",
	"=100000000000000=100000000000001",
	"=\"a\"<\"B\"",
	"=\"A\"<\"b\"",
	"=\"a\"=\"A \"",
	"=\"ab\"<\"abc\"",
	"=\"Ab\"<\"aB\"",
	"=\"a\"<>\"A\"",
	"=\"\t\"<\" \"",
	"=\"!\"<\"\"\"\"",
	"=SUM(\"\",1)",
	"=SUM(\" \",1)",
	"=SUM(\"2026-01-15\")",
	"=SUM(\"5%\")",
	"=SUM(FALSE())",
	"=SUM(1e308,1e308,-1e308)",
	"=SUM(0.1,0.2)",
	"=SUM(1,-1e-14)",
	"=AVERAGE(TRUE())",
	"=AVERAGE(\"5\")",
	"=AVERAGE(,)",
	"=AVERAGE(IF(1,))",
	"=SUM(IF(1,))",
	"=MIN()",
	"=MAX()",
	"=MIN(\"x\")",
	"=MAX(TRUE(),0)",
	"=MIN(IF(1,),5)",
	"=MAX(,-5)",
	"=MIN(1e308*10)",
	"=AVERAGE(1e308,1e308)",
	"=AVERAGE(0.1,0.2,0.3)",
	"=AVERAGE(1,2)",
	"=MAX(\"abc\")",
	"=MIN(1,\"x\",1/0)",
	"=MIN(1/0,\"x\")",
	"=SUM(\"x\",1/0)",
	"=1/0+\"x\"",
	"=\"x\"+1/0",
	"=\"x\"&1/0",
	"=1/0&\"x\"",
	"=#N/A=#VALUE!",
	"=-#N/A",
	"=#N/A%",
	"=IF(1,#N/A)",
	"=IF(#N/A,1)",
	"=AND(1,#N/A)",
	"=AND(0,#N/A)",
	"=OR(1,#N/A)",
	"=AND(\"TRUE\")",
	"=AND(\"true\",\"false\")",
	"=AND(\"\")",
	"=OR(\"\")",
	"=OR(\"\",1)",
	"=AND(IF(1,))",
	"=AND(IF(1,),1)",
	"=AND(,1)",
	"=OR(,1)",
	"=OR(,)",
	"=NOT(\"\")",
	"=NOT(\"TRUE\")",
	"=NOT(\"false\")",
	"=NOT(IF(1,))",
	"=NOT(2)",
	"=NOT(-0.5)",
	"=NOT(\"5\")",
	"=IF(\"5\",1,2)",
	"=IF(\"0\",1,2)",
	"=IF(\" \",1,2)",
	"=IF(\"FALSE\",1,2)",
	"=IF(\"True\",1,2)",
	"=IF(\"2026-01-15\",1,2)",
	"=IF(0.0000001,1,2)",
	"=IF(IF(1,),1,2)",
	"=IF(TRUE(),IF(1,),2)",
	"=IF(1,\"\")",
	"=IF(1,2,3,)",
	"=IF()",
	"=IF(1,,2)",
	"=IF(0,,2)",
	"=IF(1,TRUE())",
	"=AND(1,2,3,0)",
	"=OR(0,0,0)",
	"=TRUE(,)",
	"=FALSE()+0",
	"=\"0:00\"=IF(1,)",
	"=\"10:30\"<IF(1,)",
	"=IF(1,)<\"10:30\"",
	"=\"10:30\"<TRUE()",
	"=\"10:30\"=\"10:30:00\"",
	"=\"10:30\">\"5\"",
	"=\"5\"<\"10:30\"",
	"=\"2026-01-15\">5",
	"=5<\"abc\"",
	"=\"15/01/2026\"<\"abc\"",
	"=0.2&123456.789>\"10:30\"",
	"=LOWER(-0.5)<\"15/01/2026\"",
	"=\"10:30\">\"\"",
	"=\"10:30\">\"$5\"",
	"=\"0\">=\"15/01/2026\"",
];

/// ROUND, ABS and the text functions.
const FUNCTION_EDGES: &[&str] = &[
	"=ROUND(0.285,2)",
	"=ROUND(1.255,2)",
	"=ROUND(8.345,2)",
	"=ROUND(0.615,2)",
	"=ROUND(10.235,2)",
	"=ROUND(4.35,1)",
	"=ROUND(1.45,1)",
	"=ROUND(2.345,2)",
	"=ROUND(5.015,2)",
	"=ROUND(1.0049999999999,2)",
	"=ROUND(0.5)",
	"=ROUND(1.5)",
	"=ROUND(-0.5)",
	"=ROUND(0.49999999999999994)",
	"=ROUND(123.456,-1)",
	"=ROUND(125,-1)",
	"=ROUND(-125,-1)",
	"=ROUND(15,-1)",
	"=ROUND(1.5,0)",
	"=ROUND(1.2345678,5)",
	"=ROUND(1,-400)",
	"=ROUND(1e-300,2)",
	"=ROUND(\"2.5\")",
	"=ROUND(TRUE())",
	"=ROUND(2.5,TRUE())",
	"=ROUND(IF(1,),1)",
	"=ROUND(2.567,IF(1,))",
	"=ROUND(2.567,)",
	"=ROUND(,2)",
	"=ROUND(1/0,\"x\")",
	"=ROUND(\"x\",1/0)",
	"=ROUND(1,2,3)",
	"=ROUND(-1.005,2)",
	"=ROUND(1234.5678,-2.5)",
	"=ROUND(2.5,0.5)",
	"=ROUND(2.5,-0.5)",
	"=ROUND(1.23456,-1.5)",
	"=ROUND(0.1+0.2,16)",
	"=ROUND(0.1+0.2,17)",
	"=ROUND(1.1,308)",
	"=ROUND(1.1,309)",
	"=ROUND(1.1,22)",
	"=ROUND(1.1,23)",
	"=ABS(-0)",
	"=ABS(TRUE())",
	"=ABS(IF(1,))",
	"=ABS(\"\")",
	"=ABS(\"x\")",
	"=ABS(,)",
	"=ABS(\"$-5\")",
	"=ABS(-1e308*10)",
	"=LEN(TRUE())",
	"=LEN(IF(1,))",
	"=LEN(1/3)",
	"=LEN(1e21)",
	"=LEN()",
	"=LEN(\"a\",\"b\")",
	"=LEN(1/0)",
	"=LEFT(\"hello\",\"2\")",
	"=LEFT(\"hello\",\"x\")",
	"=LEFT(\"hello\",)",
	"=LEFT(\"hello\",IF(1,))",
	"=LEFT(\"hello\",TRUE())",
	"=LEFT(,2)",
	"=LEFT(TRUE(),2)",
	"=LEFT(1/3,4)",
	"=LEFT(\"hello\",1e308*10)",
	"=LEFT(\"hello\",-0.5)",
	"=RIGHT(\"hello\",-0.5)",
	"=RIGHT(\"hello\",1.5)",
	"=RIGHT(\"hello\",4.9)",
	"=RIGHT(\"hello\",5.5)",
	"=RIGHT(\"hello\",)",
	"=RIGHT(\"hello\",IF(1,))",
	"=LEFT(\"hello\",1,2)",
	"=LEFT()",
	"=RIGHT()",
	"=RIGHT(\"\",1)",
	"=UPPER()",
	"=UPPER(1/3)",
	"=UPPER(IF(1,))",
	"=UPPER(\"ǆ\")",
	"=LOWER(\"ΣΑΣ\")",
	"=UPPER(\"ﬃ\")",
	"=LOWER(\"İ\")",
	"=UPPER(1,2)",
	"=CONCATENATE(IF(1,),\"a\")",
	"=CONCATENATE(,\"a\")",
	"=CONCATENATE(\"a\",)",
	"=CONCATENATE(1e21)",
	"=CONCATENATE(-0)",
	"=CONCATENATE(0.1+0.2)",
	"=CONCATENATE(123456789012345680000)",
	"=CONCATENATE(1e-7,1.5e-7,0.000001234)",
	"=CONCATENATE(2^53+1)",
	"=CONCATENATE(DATE(2026,1,1))",
	"=CONCATENATE(TODAY())",
	"=CONCATENATE(\"x\",\"y\",\"z\",1,2,3)",
	"=1e300*1e10&\"\"",
	"=-0&\"\"",
	"=0.1+0.2&\"\"",
	"=(0.1+0.2)&\"\"",
	"=(0.1+0.2-0.3)&\"\"",
	"=100&\"\"",
	"=1e20&\"\"",
	"=123e-20&\"\"",
	"=\"a\"&IF(1,)&\"b\"",
	"=IF(1,)&IF(1,)",
	"=LEN(IF(1,)&IF(1,))",
];

/// DATE, YEAR, MONTH, DAY and EOMONTH at their limits.
const DATE_EDGES: &[&str] = &[
	"=YEAR(1e10)",
	"=DATE(1900,1,1)",
	"=DATE(1899,12,30)",
	"=DATE(0,1,1)",
	"=DATE(-1,1,1)",
	"=DATE(99,1,1)",
	"=DATE(100,1,1)",
	"=DATE(1899,1,1)",
	"=DATE(1900,0,1)",
	"=DATE(1900,1,0)",
	"=DATE(1900,1,-1)",
	"=DATE(1899,13,1)",
	"=DATE(9999,12,31)",
	"=DATE(9999,12,32)",
	"=DATE(9999,13,1)",
	"=DATE(2026,1,100000)",
	"=DATE(2026,1,10000000)",
	"=DATE(2026,1,-100000)",
	"=DATE(2026,-100,1)",
	"=DATE(2026,1000,1)",
	"=DATE(\"2026\",\"1\",\"15\")",
	"=DATE(TRUE(),1,1)",
	"=DATE(,1,1)",
	"=DATE(2026,,1)",
	"=DATE(2026,1,)",
	"=DATE(2026,1)",
	"=DATE(2026,1,1,1)",
	"=DATE(\"x\",1,1)",
	"=DATE(-0.5,1,1)",
	"=DATE(2026,-0.5,1)",
	"=DATE(2026,1,-0.5)",
	"=DATE(2026,1,0.5)",
	"=DATE(1e10,1,1)",
	"=DATE(2026,1e10,1)",
	"=DATE(2026,1,1e10)",
	"=DATE(1900,3,1)",
	"=DATE(1900,2,28)",
	"=YEAR(0.5)",
	"=YEAR(-0.5)",
	"=YEAR(2958465)",
	"=YEAR(2958466)",
	"=MONTH(1e15)",
	"=DAY(1e15)",
	"=YEAR(\"x\")",
	"=YEAR(\"\")",
	"=YEAR()",
	"=YEAR(,)",
	"=YEAR(IF(1,))",
	"=YEAR(TRUE())",
	"=YEAR(\"10:30\")",
	"=YEAR(\"5\")",
	"=DAY(-0.0000001)",
	"=DAY(\"2026-01-15 23:59\")",
	"=YEAR(1,2)",
	"=EOMONTH(46037,0)",
	"=EOMONTH(46037,\"1\")",
	"=EOMONTH(\"2026-01-15\",0)",
	"=EOMONTH(46037.9,0)",
	"=EOMONTH(0,0)",
	"=EOMONTH(0,-1)",
	"=EOMONTH(1,0)",
	"=EOMONTH(31,-1)",
	"=EOMONTH(32,-1)",
	"=EOMONTH(60,0)",
	"=EOMONTH(46037,)",
	"=EOMONTH(,0)",
	"=EOMONTH(46037)",
	"=EOMONTH(46037,TRUE())",
	"=EOMONTH(46037,-0.5)",
	"=EOMONTH(46037,2.0000000000000004)",
	"=EOMONTH(46037,1e10)",
	"=EOMONTH(2958465,0)",
	"=EOMONTH(2958435,0)",
	"=EOMONTH(2958434,0)",
	"=EOMONTH(2958466,-1)",
	"=EOMONTH(1e10,0)",
	"=EOMONTH(46037,-1500)",
	"=EOMONTH(46037,-1513)",
	"=EOMONTH(46037,-1514)",
	"=EOMONTH(-0.5,0)",
	"=EOMONTH(0.5,-1)",
	"=DATE(2026,1,1)+0.5",
	"=TODAY()-DATE(2026,1,1)",
	"=DATE(2026,11,9)=TODAY()",
	"=EOMONTH(\"x\",0)",
	"=EOMONTH(1/0,\"x\")",
	"=DATE(1900,-22800,1)",
	"=DATE(1900,-22788,1)",
	"=DATE(-1900,1,1)",
	"=DATE(-1899,1,1)",
	"=DATE(-319,1,1)",
	"=DATE(-318,1,1)",
	"=DATE(-319,13,1)",
	"=DATE(-320,13,1)",
	"=DATE(1900,-3828,1)",
	"=DATE(0,1,-693594)",
	"=DATE(-1e10,1,1)",
	"=DATE(-1,12,31)",
	"=EOMONTH(-1,\"a b\")",
	"=DAY(EOMONTH(-2.5,#DIV/0!))",
	"=EOMONTH(\"7e69\",0)",
	"=EOMONTH(1e16,-1)",
	"=EOMONTH(0,-4800)",
];

/// Actual's query functions, arrays as categories, arity, and export rounding.
const QUERY_EDGES: &[&str] = &[
	"={,1}",
	"={1,}",
	"={}",
	"=QUERY(1)",
	"=QUERY(TRUE())",
	"=QUERY(IF(1,))",
	"=QUERY(,)",
	"=QUERY(1/0)",
	"=QUERY_EXTRACT_TIMEFRAME_START(1/3)",
	"=QUERY_EXTRACT_TIMEFRAME_START(\"x\")",
	"=QUERY_EXTRACT_TIMEFRAME_START(\"x\")&\"\"",
	"=QUERY_EXTRACT_TIMEFRAME_START()",
	"=BUDGET_QUERY(\"spent\",\"\",\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",,\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",1,\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",\"x\",,)",
	"=BUDGET_QUERY(,\"x\",\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",\"x\",2026,\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",\"x\",TRUE(),\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",{1,2},\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",{1/0},\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",{\"a\",1/0},\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",{\"a\";TRUE()},\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",IF(1,),\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",1+1,\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",\"a\"&\"b\",\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",QUERY_EXTRACT_CATEGORIES(\"x\"),\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",(QUERY_EXTRACT_CATEGORIES(\"x\")),\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",\"x\",\"2026-01\",1/0)",
	"=BUDGET_QUERY(1/0,1/0,1/0,1/0)",
	"=BUDGET_QUERY(\"bogus\",1/0,\"x\",\"y\")",
	"=BUDGET_QUERY(\"bogus\",\"x\",\"2026-01\",\"2026-02\")",
	"=IF(1/0)",
	"=ABS(1/0,2)",
	"=LEFT(1/0,1,2)",
	"=DATE(1/0,1)",
	"=1e-300",
	"=1e-308",
	"=1e-310",
	"=1e-320*1",
	"=-1e-310",
	"=1e-310&\"\"",
	"=1.7976931348623157e308",
	"=-1.7976931348623157e308",
	"=123456.789e3",
	"=0.1*0.1",
	"=9.99999999995",
	"=9.999999999949",
	"=0.000099999999999",
	"=99999999999.5",
	"=-99999999999.5",
	"=0.5e-5",
	"=1/3*-1",
	"=BUDGET_QUERY(\"spent\",QUERY_EXTRACT_CATEGORIES(),\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",\"x\",\"2026-01\")",
	"=BUDGET_QUERY(\"spent\",\"x\",\"2026-01\",\"2026-02\",1)",
	"=BUDGET_QUERY(\"spent\",\"x\",\"2026-01\",\"2026-02\")+1/0",
];

/// Laziness, error order, collation, and very large or small results.
const MIXED_EDGES: &[&str] = &[
	"=1e300",
	"=ROUND(1e300,0)",
	"=ROUND(1e300,1)",
	"=ROUND(1e300,2)",
	"=ROUND(1e300,3)",
	"=1e300*1",
	"=1.2345678901234e200",
	"=2^100",
	"=7^26",
	"=IF(TRUE(),1,FOO(1))",
	"=IF(TRUE(),1,foo)",
	"=IF(TRUE(),1,SQRT(-1))",
	"=IF(1,2,#LIC!)",
	"=IF(1,2,#FOO!)",
	"=é",
	"=SUMé(1)",
	"=é1",
	"=SUM(1e308,1e308,#N/A)",
	"=SUM(1e308,1e308,\"x\")",
	"=DATE(10000,-12,1)",
	"=DATE(10000,-11,1)",
	"=DATE(-1,24,1)",
	"=+IF(1,)",
	"=+TRUE()",
	"=-IF(1,)",
	"=IF(1,)%",
	"=IF(1,)+0",
	"=IF(1,)*1",
	"=MONTH(123456789012345)",
	"=DAY(123456789012345)",
	"=YEAR(123456789012345)",
	"=MONTH(98765432109)",
	"=DAY(98765432109)",
	"=\"a\"<\"_\"",
	"=\"0\"<\"$\"",
	"=\"\n\"<\"\t\"",
	"=\"\r\"<\" \"",
	"=\"[\"<\"(\"",
	"=\"@\"<\"*\"",
	"=\"#\"<\"&\"",
	"=\"`\"<\"%\"",
	"=\"9\"<\"a\"",
	"=\"co-op\"<\"coop\"",
	"=\"a\"=\"a\"",
	"=\"aa\"<\"aB\"",
	"=\"\\\"<\"/\"",
	"=SUM(\"$1\",\"2%\",\"10:30\")",
	"=IF(1,2,3)=2",
	"=SUM(1)(2)",
	"=1+2=3",
	"=\"a\"&\"b\"=\"ab\"",
	"=2*3^2",
	"=-2*-2",
	"=10/2/5",
	"=2^-1",
	"=1/3*3=1",
	"=(1/3)*3",
	"=\"1\"&2+3",
	"=TRUE()+\"1\"",
	"=MAX(\"\",1)",
	"=MIN(\"\")",
	"=AVERAGE(\"\")",
	"=AVERAGE(\"\",2)",
	"=CONCATENATE(\"\")",
	"=LEN(\" \")",
	"=IF(\"\",1)",
	"=1e299",
	"=1e250",
	"=1e200",
	"=1e100",
	"=1e50",
	"=1e30",
	"=1e22",
	"=3e300",
	"=7e250",
	"=1e-200",
	"=1e-250",
	"=1e-290",
	"=1e-298",
	"=1e-299",
	"=1e-297",
	"=123456789012345",
	"=2^200",
	"=2^500",
	"=2^1000",
	"=-2^1001",
];

/// What Veritas refuses where Actual answers: references, arrays outside
/// BUDGET_QUERY, non-ASCII text comparisons and serials past 10^15.
const REFUSED_EDGES: &[&str] = &[
	"=RC1",
	"=foo1",
	"=a1",
	"=$A$1",
	"=A$1",
	"=XFD1",
	"=ZZZ1",
	"=AAAA1",
	"=A0",
	"=A40001",
	"=A40000",
	"=1:2",
	"=A:A",
	"=E3",
	"=e3",
	"=\"a\"<\"á\"",
	"=YEAR(1e300)",
	"=BUDGET_QUERY(\"spent\",{\"a\"}&\"b\",\"2026-01\",\"2026-02\")",
	"=BUDGET_QUERY(\"spent\",IF(1,QUERY_EXTRACT_CATEGORIES(\"x\")),\"2026-01\",\"2026-02\")",
	"={1}",
	"={\"a\"}",
	"={1;2}",
	"={1,2;3,4}",
	"={-1}",
	"={1+1}",
	"={TRUE()}",
	"={\"a\",1/0}",
	"=QUERY({\"all\"})",
	"=SUM(QUERY_EXTRACT_CATEGORIES(\"x\"))",
	"=QUERY_EXTRACT_CATEGORIES(\"x\")&\"z\"",
	"=QUERY_EXTRACT_CATEGORIES()",
	"=QUERY_EXTRACT_CATEGORIES(1/0)",
	"=LEN(QUERY_EXTRACT_CATEGORIES(\"x\"))",
	"=QUERY_EXTRACT_CATEGORIES(\"x\",1)",
	"=SUM({1})",
	"=-{1}",
	"={1}+1",
	"=YEAR(1e16)",
	"=MONTH(1e16)",
	"=DAY(1e16)",
	"=YEAR(1e17)",
	"=MONTH(1e17)",
	"=DAY(1e17)",
	"=YEAR(1e18)",
	"=MONTH(1e18)",
	"=DAY(1e18)",
	"=YEAR(9007199254740993)",
];

#[test]
fn d73_formula_card_matches_actual() {
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

/// Black-box probes of the clean-room engine (D75) at the edges HyperFormula's
/// docs leave open, each compared with Actual's card.
#[test]
fn d75_formula_engine_edge_cases_match_actual() {
	let budget = fixture("all-features");
	let mut cases: Vec<Case> = [
		GRAMMAR_EDGES,
		TEXT_AS_NUMBER_EDGES,
		LOGIC_EDGES,
		FUNCTION_EDGES,
		DATE_EDGES,
		QUERY_EDGES,
		MIXED_EDGES,
	]
	.concat()
	.into_iter()
	.map(|text| formula(text, Expect::Same))
	.collect();
	cases.extend(
		REFUSED_EDGES
			.iter()
			.map(|text| formula(text, Expect::Refused)),
	);
	compare(&budget, &cases);
}

/// HyperFormula evaluates every operand and argument, IF's unused branch
/// included, so a QUERY anywhere is fetched; SUM, AVERAGE, MIN and MAX stop
/// at an error, and an unknown function evaluates nothing.
#[test]
fn d75_formula_engine_fetches_queries_from_every_branch() {
	let budget = fixture("all-features");
	let queries = queries(&budget);
	let cases: Vec<Case> = [
		"=IF(QUERY(\"static\")<>0, QUERY(\"all\"), 0)",
		"=IF(QUERY(\"static\")=0, 0, QUERY(\"all\"))",
		"=IF(QUERY(\"static\")<>0, QUERY_EXTRACT_TIMEFRAME_START(\"days\"), \"none\")",
		"=IF(QUERY(\"static\")>0, BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(\"groceries\"), \"2026-01\", \"2026-06\"), 0)",
		"=IF(QUERY(\"static\")<>0, BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(\"groceries\"), QUERY_EXTRACT_TIMEFRAME_START(\"static\"), \"2026-06\"), 2)",
		"=1/QUERY(\"static\")+QUERY(\"payee\")",
		"=IF(1/QUERY(\"static\"), QUERY(\"payee\"), 2)",
		"=AND(1/QUERY(\"static\"), QUERY(\"payee\"))",
		"=CONCATENATE(1/QUERY(\"static\"), QUERY(\"payee\"))",
		"=ROUND(1/QUERY(\"static\"), QUERY(\"payee\"))",
		"=LEFT(1/QUERY(\"static\"), -QUERY(\"payee\"))",
		"=DATE(1/QUERY(\"static\")+2026, 1, QUERY(\"payee\"))",
		"=EOMONTH(1/QUERY(\"static\")+46037, QUERY(\"payee\")/21600)",
		"=BUDGET_QUERY(IF(1/QUERY(\"static\")<>0,\"spent\"), QUERY_EXTRACT_CATEGORIES(\"groceries\"), \"2026-01\", \"2026-06\")",
		"=BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(IF(1/QUERY(\"static\")<>0,\"groceries\")), QUERY_EXTRACT_TIMEFRAME_START(\"static\"), \"2026-06\")",
		"=BUDGET_QUERY(\"spent\", QUERY_EXTRACT_CATEGORIES(\"gro\"&\"ceries\"), \"2026-01\", \"2026-06\")",
		"=SUM(1/QUERY(\"static\"), QUERY(\"payee\"))",
		"=MAX(1/QUERY(\"static\"), QUERY(\"payee\"))",
		"=IF(TRUE(), SUM(1/QUERY(\"static\"), QUERY(\"payee\")), FOO(QUERY(\"payee\")))",
		"=IF(TRUE(), SUM(1/QUERY(\"static\"), QUERY(\"payee\")), ABS(QUERY(\"payee\"),1))",
		"=IF(TRUE(), SUM(1/QUERY(\"static\"), QUERY(\"payee\")), SUM(1/0,QUERY(\"payee\")))",
		"=IF(TRUE(), SUM(1/QUERY(\"static\"), QUERY(\"payee\")), #N/A+QUERY(\"payee\"))",
	]
	.iter()
	.map(|text| with_queries(text, &queries))
	.collect();
	compare(&budget, &cases);
}

/// A small random formula over D75's functions, built from pieces Actual's
/// card accepts and Veritas promises to match: no cell references or arrays,
/// ASCII text only, and date functions only over real calendar dates.
struct Generator {
	state: u64,
}

impl Generator {
	fn below(&mut self, n: u64) -> u64 {
		self.state = self
			.state
			.wrapping_mul(6_364_136_223_846_793_005)
			.wrapping_add(1_442_695_040_888_963_407);
		(self.state >> 33) % n
	}

	fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
		items[usize::try_from(self.below(items.len() as u64)).unwrap()]
	}

	fn date(&mut self) -> String {
		format!(
			"DATE({}, {}, {})",
			2000 + self.below(31),
			self.below(14),
			self.below(33)
		)
	}

	fn query(&mut self) -> String {
		let name = self.pick(&[
			"all",
			"static",
			"groceries",
			"everyday",
			"lastYear",
			"yearToDate",
			"diningOrRent",
			"missing",
		]);
		match self.below(3) {
			0 => format!("QUERY(\"{name}\")"),
			1 => format!(
				"BUDGET_QUERY(\"{}\", QUERY_EXTRACT_CATEGORIES(\"{name}\"), QUERY_EXTRACT_TIMEFRAME_START(\"{name}\"), QUERY_EXTRACT_TIMEFRAME_END(\"{name}\"))",
				self.pick(&["budgeted", "spent", "balance_start", "balance_end", "goal"])
			),
			_ => format!("QUERY_EXTRACT_TIMEFRAME_START(\"{name}\")"),
		}
	}

	fn leaf(&mut self) -> String {
		match self.below(8) {
			0 => self
				.pick(&[
					"0",
					"1",
					"2",
					"-3",
					"7",
					"100",
					"12.5",
					"0.1",
					"1e3",
					"50%",
					"-0.25",
					"1234567.891",
				])
				.to_owned(),
			1 => self
				.pick(&[
					"\"abc\"",
					"\"5\"",
					"\"\"",
					"\" 12 \"",
					"\"2026-01-15\"",
					"\"TRUE\"",
					"\"a b\"",
					"\"-4.5\"",
				])
				.to_owned(),
			2 => self.pick(&["TRUE()", "FALSE()", "TODAY()"]).to_owned(),
			3 => self.date(),
			4 => self.query(),
			5 => self.pick(&["1/0", "#N/A", "\"x\"+1"]).to_owned(),
			_ => format!("{}", self.below(50)),
		}
	}

	fn expr(&mut self, depth: u32) -> String {
		if depth == 0 || self.below(4) == 0 {
			return self.leaf();
		}
		let a = self.expr(depth - 1);
		let b = self.expr(depth - 1);
		match self.below(14) {
			0 | 1 => format!(
				"({a}{}{b})",
				self.pick(&[
					"+", "-", "*", "/", "^", "&", "=", "<>", "<", ">", "<=", ">="
				])
			),
			2 => format!("-{a}"),
			3 => {
				let c = self.expr(depth - 1);
				format!("IF({a}, {b}, {c})")
			}
			4 => format!(
				"{}({a}, {b})",
				self.pick(&["AND", "OR", "SUM", "AVERAGE", "MIN", "MAX", "CONCATENATE"])
			),
			5 => format!(
				"{}({a})",
				self.pick(&["NOT", "ABS", "LEN", "UPPER", "LOWER", "SUM", "ROUND"])
			),
			6 => format!("ROUND({a}, {})", self.below(5)),
			7 => format!("{}({a}, {})", self.pick(&["LEFT", "RIGHT"]), self.below(6)),
			8 => format!("{}({})", self.pick(&["YEAR", "MONTH", "DAY"]), self.date()),
			9 => format!(
				"EOMONTH({}, {})",
				self.date(),
				i64::try_from(self.below(25)).unwrap() - 12
			),
			10 => format!(
				"{}({})",
				self.pick(&["YEAR", "MONTH", "DAY"]),
				self.below(80_000)
			),
			11 => format!("({a}{}{b})", self.pick(&["+", "*", "&"])),
			_ => format!("IF({a}, {b})"),
		}
	}
}

/// An independent random sweep of D75's grammar against Actual's card:
/// 1,500 formulas over the all-features budget, each the same value or the
/// same error message as Actual, with nothing refused.
#[test]
fn d75_random_formulas_over_the_subset_match_actual() {
	let budget = fixture("all-features");
	let queries = queries(&budget);
	let mut generator = Generator {
		state: 0x0005_eed0_fd75_u64,
	};
	let cases: Vec<Case> = (0..1_500)
		.map(|_| {
			let depth = u32::try_from(generator.below(4)).unwrap() + 1;
			with_queries(&format!("={}", generator.expr(depth)), &queries)
		})
		.collect();
	let sheet = EnvelopeSheet::compute(&budget, CivilDate::from(today())).unwrap();
	let mut kinds = [0_usize; 5];
	for case in &cases {
		let card = formula_card(&budget, &sheet, meta(&case.meta).as_ref(), today()).unwrap();
		kinds[match card.value {
			FormulaValue::Number(_) => 0,
			FormulaValue::Text(_) => 1,
			FormulaValue::Boolean(_) => 2,
			FormulaValue::Error(_) => 3,
			FormulaValue::Empty => 4,
		}] += 1;
	}
	assert!(
		kinds[..4].iter().all(|&count| count >= 100),
		"the sweep reaches numbers, text, booleans and errors alike: {kinds:?}"
	);
	compare(&budget, &cases);
}

/// D75's exclusions beyond the list above: functions HyperFormula has that
/// Actual's card computes, which Veritas must show as an unknown function
/// (`Formula error: NAME`), never as a value of its own, in any letter case.
/// In a branch IF doesn't take, it changes nothing, in either app.
#[test]
fn d75_excluded_functions_actual_computes_show_name_in_veritas() {
	let budget = fixture("all-features");
	let cases: Vec<Case> = [
		"=PI()",
		"=MOD(5,2)",
		"=TRIM(\" a \")",
		"=SUBSTITUTE(\"aa\",\"a\",\"b\")",
		"=FIND(\"b\",\"abc\")",
		"=REPT(\"a\",3)",
		"=CEILING(2.5,1)",
		"=FLOOR(2.5,1)",
		"=WEEKDAY(46037)",
		"=TRUNC(2.7)",
		"=SIGN(-2)",
		"=PRODUCT(2,3)",
		"=MEDIAN(1,2,3)",
		"=EXACT(\"a\",\"a\")",
		"=CHAR(65)",
		"=CODE(\"A\")",
		"=ISNUMBER(1)",
		"=COUNTA(1,2)",
		"=PROPER(\"ab\")",
		"=sqrt(4)",
		"=Power(2,3)",
		"=SUM(1, SQRT(4))",
	]
	.iter()
	.map(|text| formula(text, Expect::OutOfScope))
	.collect();
	let mut cases = cases;
	cases.push(formula("=IF(TRUE(), 1, MOD(5,2))", Expect::Same));
	cases.push(formula("=IF(FALSE(), MOD(5,2), \"kept\")", Expect::Same));
	compare(&budget, &cases);
}

/// Results from 10^15 to 10^25, where the 11-digit export's floating-point
/// division shows in the last bit (`=7^26` exports 9.387480337600001e21).
#[test]
fn d75_formula_engine_exports_large_results_as_actual_does() {
	let budget = fixture("all-features");
	let mut generator = Generator { state: 26 };
	let mut cases = Vec::new();
	for base in 2..=40u64 {
		for exponent in [13, 17, 19, 23, 26, 31] {
			let power = (base as f64).powi(exponent);
			if (1e15..1e25).contains(&power) {
				cases.push(formula(&format!("={base}^{exponent}"), Expect::Same));
			}
		}
	}
	for _ in 0..400 {
		let digits = 1_000_000 + generator.below(9_000_000);
		let scale = 9 + generator.below(10);
		let factor = 1 + generator.below(999_999);
		cases.push(formula(
			&format!("={digits}e{scale}*{factor}"),
			Expect::Same,
		));
	}
	compare(&budget, &cases);
}
