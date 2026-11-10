//! R-BUD-3 as a person uses it: a simulated amount field that behaves like
//! Actual's `InputValue` (focus shows the edit text selected, every keystroke
//! goes through the single-operator rule, Return commits the field's text).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use veritas_domain::Money;
use veritas_engine::{
	AmountEdit, MAX_SAFE_MINOR, NumberFormat, Rejection, commit_amount, edit_text,
	single_operator_rule,
};

use NumberFormat::{ApostropheDot, CommaDot, CommaDotIn, DotComma, SpaceComma};

const FORMATS: [NumberFormat; 5] = [CommaDot, DotComma, SpaceComma, ApostropheDot, CommaDotIn];

struct Field {
	current: Money,
	format: NumberFormat,
	original: String,
	text: String,
	selected: bool,
}

impl Field {
	fn click(minor: i64, format: NumberFormat) -> Self {
		let current = Money::from_minor(minor);
		let original = edit_text(current, format, false);
		Field {
			current,
			format,
			text: original.clone(),
			original,
			selected: true,
		}
	}

	fn shows(&self) -> &str {
		&self.text
	}

	fn change_to(&mut self, inserted: &str) {
		let new_text = if self.selected {
			inserted.to_owned()
		} else {
			format!("{}{inserted}", self.text)
		};
		self.selected = false;
		self.text = single_operator_rule(&self.original, &new_text);
	}

	fn type_keys(&mut self, keys: &str) -> &mut Self {
		for key in keys.chars() {
			self.change_to(&key.to_string());
		}
		self
	}

	fn paste(&mut self, clipboard: &str) -> &mut Self {
		self.change_to(clipboard);
		self
	}

	fn press_end(&mut self) -> &mut Self {
		self.selected = false;
		self
	}

	fn backspace(&mut self) -> &mut Self {
		if self.selected {
			self.text.clear();
		} else {
			self.text.pop();
		}
		self.selected = false;
		self
	}

	fn press_return(&self) -> AmountEdit {
		commit_amount(self.current, &self.text, self.format)
	}
}

fn saved(minor: i64) -> AmountEdit {
	AmountEdit {
		amount: Money::from_minor(minor),
		rejection: None,
	}
}

fn kept(minor: i64, rejection: Rejection) -> AmountEdit {
	AmountEdit {
		amount: Money::from_minor(minor),
		rejection: Some(rejection),
	}
}

#[test]
fn clicking_100_and_typing_plus_12_34_saves_112_34() {
	let mut field = Field::click(10_000, CommaDot);
	assert_eq!(field.shows(), "100.00");
	field.type_keys("+");
	assert_eq!(field.shows(), "100.00+");
	field.type_keys("12.34");
	assert_eq!(field.shows(), "100.00+12.34");
	assert_eq!(field.press_return(), saved(11_234));
}

#[test]
fn typing_minus_over_the_selection_subtracts() {
	let mut field = Field::click(10_000, CommaDot);
	field.type_keys("-5");
	assert_eq!(field.shows(), "100.00-5");
	assert_eq!(field.press_return(), saved(9_500));
}

#[test]
fn appending_after_moving_to_the_end_adds_to_the_value() {
	let mut field = Field::click(10_000, CommaDot);
	field.press_end().type_keys("+12.34");
	assert_eq!(field.press_return(), saved(11_234));
}

#[test]
fn typing_a_fresh_number_over_the_selection_replaces_it() {
	assert_eq!(
		Field::click(10_000, CommaDot)
			.type_keys("250")
			.press_return(),
		saved(25_000)
	);
	assert_eq!(
		Field::click(10_000, CommaDot)
			.backspace()
			.type_keys("42.5")
			.press_return(),
		saved(4_250)
	);
}

#[test]
fn pasting_an_amount_with_a_currency_symbol_and_grouping_replaces_the_value() {
	assert_eq!(
		Field::click(10_000, CommaDot)
			.paste("$1,234.56")
			.press_return(),
		saved(123_456)
	);
	assert_eq!(
		Field::click(10_000, DotComma)
			.paste("\u{20AC}1.234,56")
			.press_return(),
		saved(123_456)
	);
	assert_eq!(
		Field::click(10_000, SpaceComma)
			.paste("1\u{A0}234,56\u{A0}\u{20AC}")
			.press_return(),
		saved(123_456)
	);
	assert_eq!(
		Field::click(10_000, CommaDot)
			.press_end()
			.paste("+$1,234.56")
			.press_return(),
		saved(133_456)
	);
}

#[test]
fn appending_to_a_large_value_is_exact_to_the_cent() {
	let mut field = Field::click(987_654_321_099, CommaDot);
	assert_eq!(field.shows(), "9,876,543,210.99");
	field.type_keys("+0.01");
	assert_eq!(field.press_return(), saved(987_654_321_100));

	let mut field = Field::click(MAX_SAFE_MINOR - 1, CommaDot);
	field.type_keys("+0.01");
	assert_eq!(field.press_return(), saved(MAX_SAFE_MINOR));

	let mut field = Field::click(MAX_SAFE_MINOR, CommaDot);
	field.type_keys("+0.01");
	assert_eq!(
		field.press_return(),
		kept(MAX_SAFE_MINOR, Rejection::TooLarge)
	);
}

#[test]
fn the_append_journey_works_in_every_number_format() {
	for format in FORMATS {
		let decimal = format.decimal_separator();
		let mut field = Field::click(123_456_789, format);
		field.type_keys(&format!("+12{decimal}34"));
		assert_eq!(field.press_return(), saved(123_458_023), "{format:?}");
	}
}

/// Tabbing through a cell commits it, so focusing and pressing Return must
/// never change a value that only displays.
#[test]
fn committing_an_untouched_positive_field_keeps_its_value_in_every_format() {
	let values = [
		0,
		1,
		99,
		10_000,
		123_456,
		100_000_000,
		987_654_321_099,
		MAX_SAFE_MINOR,
	];
	for format in FORMATS {
		for minor in values {
			assert_eq!(
				Field::click(minor, format).press_return(),
				saved(minor),
				"{format:?} {minor}"
			);
		}
	}
}

#[test]
fn invalid_input_keeps_the_old_value() {
	assert_eq!(
		Field::click(10_000, CommaDot)
			.type_keys("(5")
			.press_return(),
		kept(10_000, Rejection::NotAnAmount)
	);
	assert_eq!(
		Field::click(10_000, CommaDot)
			.type_keys("abc")
			.press_return(),
		kept(10_000, Rejection::NotAnAmount)
	);
	assert_eq!(
		Field::click(10_000, CommaDot).type_keys("+").press_return(),
		kept(10_000, Rejection::NotAnAmount)
	);
	assert_eq!(
		Field::click(10_000, CommaDot).backspace().press_return(),
		kept(10_000, Rejection::Empty)
	);
}

#[test]
fn changing_your_mind_after_an_operator_keeps_the_value() {
	let mut field = Field::click(10_000, CommaDot);
	field.type_keys("+").backspace();
	assert_eq!(field.shows(), "100.00");
	assert_eq!(field.press_return(), saved(10_000));
}

#[test]
fn dividing_by_zero_keeps_the_old_value() {
	assert_eq!(
		Field::click(10_000, CommaDot)
			.type_keys("/0")
			.press_return(),
		kept(10_000, Rejection::NotFinite)
	);
	assert_eq!(
		Field::click(10_000, CommaDot)
			.type_keys("5/(1-1)")
			.press_return(),
		kept(10_000, Rejection::NotFinite)
	);
}

#[test]
fn a_negative_value_shows_its_sign_and_appends_from_it() {
	let mut field = Field::click(-5_000, CommaDot);
	assert_eq!(field.shows(), "-50.00");
	field.type_keys("+12.34");
	assert_eq!(field.shows(), "-50.00+12.34");
	assert_eq!(field.press_return(), saved(-3_766));
}

#[test]
fn committing_an_untouched_negative_field_keeps_its_value() {
	for format in FORMATS {
		assert_eq!(
			Field::click(-4_500, format).press_return(),
			saved(-4_500),
			"{format:?}"
		);
	}
}

#[test]
fn pasting_a_negative_amount_replaces_the_value() {
	assert_eq!(
		Field::click(10_000, CommaDot)
			.paste("-45.00")
			.press_return(),
		saved(-4_500)
	);
	assert_eq!(
		Field::click(-5_000, CommaDot)
			.paste("-$1,234.56")
			.press_return(),
		saved(-123_456)
	);
}

#[test]
fn typing_minus_over_a_negative_value_subtracts_from_it() {
	let mut field = Field::click(-5_000, CommaDot);
	field.type_keys("-5");
	assert_eq!(field.shows(), "-50.00-5");
	assert_eq!(field.press_return(), saved(-5_500));
}
