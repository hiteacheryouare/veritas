//! JSON text in JavaScript's key order. `serde_json` sorts object keys, but
//! Actual writes rule JSON with `JSON.stringify`, which keeps insertion order.
//! Editing a schedule's rule has to rewrite only what changed, so these
//! helpers split and rebuild the text instead of reparsing it.

fn skip_space(bytes: &[u8], mut i: usize) -> usize {
	while bytes
		.get(i)
		.is_some_and(|b| matches!(b, b' ' | b'\t' | b'\n' | b'\r'))
	{
		i += 1;
	}
	i
}

/// The index just past the JSON value that starts at `i`.
fn value_end(bytes: &[u8], i: usize) -> Option<usize> {
	match bytes.get(i)? {
		b'"' => {
			let mut j = i + 1;
			loop {
				match bytes.get(j)? {
					b'\\' => j += 2,
					b'"' => return Some(j + 1),
					_ => j += 1,
				}
			}
		}
		b'{' | b'[' => {
			let mut depth = 0usize;
			let mut j = i;
			loop {
				match bytes.get(j)? {
					b'"' => {
						j = value_end(bytes, j)?;
						continue;
					}
					b'{' | b'[' => depth += 1,
					b'}' | b']' => {
						depth -= 1;
						if depth == 0 {
							return Some(j + 1);
						}
					}
					_ => {}
				}
				j += 1;
			}
		}
		_ => {
			let mut j = i;
			while bytes
				.get(j)
				.is_some_and(|b| !matches!(b, b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r'))
			{
				j += 1;
			}
			(j > i).then_some(j)
		}
	}
}

/// The raw text of each element of a top-level JSON array.
pub(crate) fn array_items(text: &str) -> Option<Vec<&str>> {
	let bytes = text.as_bytes();
	let mut i = skip_space(bytes, 0);
	if bytes.get(i) != Some(&b'[') {
		return None;
	}
	i = skip_space(bytes, i + 1);
	let mut items = Vec::new();
	if bytes.get(i) == Some(&b']') {
		return Some(items);
	}
	loop {
		let end = value_end(bytes, i)?;
		items.push(text.get(i..end)?);
		i = skip_space(bytes, end);
		match bytes.get(i)? {
			b',' => i = skip_space(bytes, i + 1),
			b']' => return Some(items),
			_ => return None,
		}
	}
}

/// The decoded key and raw value text of each member of a JSON object.
pub(crate) fn object_members(text: &str) -> Option<Vec<(String, &str)>> {
	let bytes = text.as_bytes();
	let mut i = skip_space(bytes, 0);
	if bytes.get(i) != Some(&b'{') {
		return None;
	}
	i = skip_space(bytes, i + 1);
	let mut members = Vec::new();
	if bytes.get(i) == Some(&b'}') {
		return Some(members);
	}
	loop {
		let key_end = value_end(bytes, i)?;
		let key: String = serde_json::from_str(text.get(i..key_end)?).ok()?;
		i = skip_space(bytes, key_end);
		if bytes.get(i) != Some(&b':') {
			return None;
		}
		i = skip_space(bytes, i + 1);
		let end = value_end(bytes, i)?;
		members.push((key, text.get(i..end)?));
		i = skip_space(bytes, end);
		match bytes.get(i)? {
			b',' => i = skip_space(bytes, i + 1),
			b'}' => return Some(members),
			_ => return None,
		}
	}
}

/// `{ ...object, [key]: value }` as `JSON.stringify` writes it: the key keeps
/// its place, or goes last when it's new.
pub(crate) fn with_member(object: &str, key: &str, raw_value: &str) -> Option<String> {
	let mut members: Vec<(String, String)> = object_members(object)?
		.into_iter()
		.map(|(name, value)| (name, value.to_owned()))
		.collect();
	match members.iter_mut().find(|(name, _)| name == key) {
		Some(member) => member.1 = raw_value.to_owned(),
		None => members.push((key.to_owned(), raw_value.to_owned())),
	}
	Some(write_object(
		members
			.iter()
			.map(|(name, value)| (name.as_str(), value.as_str())),
	))
}

pub(crate) fn write_object<'a>(members: impl IntoIterator<Item = (&'a str, &'a str)>) -> String {
	let body: Vec<String> = members
		.into_iter()
		.map(|(key, value)| format!("{}:{value}", string(key)))
		.collect();
	format!("{{{}}}", body.join(","))
}

pub(crate) fn write_array(items: impl IntoIterator<Item = String>) -> String {
	format!("[{}]", items.into_iter().collect::<Vec<_>>().join(","))
}

/// A JSON string. `serde_json` escapes exactly what `JSON.stringify` does
/// for valid Unicode.
pub(crate) fn string(text: &str) -> String {
	serde_json::Value::String(text.to_owned()).to_string()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn splits_without_reordering() {
		let text =
			r#"[{"op":"is","field":"description","value":"a,b"} , {"z":1,"a":[1,{"x":"]"}]},null]"#;
		let items = array_items(text).unwrap();
		assert_eq!(
			items,
			[
				r#"{"op":"is","field":"description","value":"a,b"}"#,
				r#"{"z":1,"a":[1,{"x":"]"}]}"#,
				"null"
			]
		);
		let members = object_members(items[1]).unwrap();
		assert_eq!(
			members,
			[("z".to_owned(), "1"), ("a".to_owned(), r#"[1,{"x":"]"}]"#)]
		);
		assert_eq!(array_items("[]").unwrap(), Vec::<&str>::new());
		assert!(array_items("{}").is_none() && array_items("[1,").is_none());
	}

	#[test]
	fn with_member_keeps_the_key_in_place() {
		let item = r#"{"op":"is","field":"acct","value":"old","type":"id"}"#;
		assert_eq!(
			with_member(item, "value", r#""new""#).unwrap(),
			r#"{"op":"is","field":"acct","value":"new","type":"id"}"#
		);
		assert_eq!(
			with_member(r#"{"op":"is"}"#, "value", "null").unwrap(),
			r#"{"op":"is","value":null}"#
		);
		assert_eq!(string("é\"\n"), r#""é\"\n""#);
	}
}
