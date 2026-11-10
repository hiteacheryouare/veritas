import assert from 'node:assert/strict';
import { test } from 'node:test';

import { compareCodePoints, formatReal, sqlReal, toCanonical } from '../src/canonicalJson.ts';

test('SQLite storage classes stay distinguishable', () => {
	const text = toCanonical({
		rows: [{ integer: 16384n, real: sqlReal(16384), fraction: sqlReal(0.5), text: '16384', blob: Uint8Array.of(0, 255), none: null }]
	});
	assert.equal(
		text,
		'{\n\t"rows": [\n\t\t{"blob":{"$blob":"00ff"},"fraction":0.5,"integer":16384,"none":null,"real":16384.0,"text":"16384"}\n\t]\n}\n'
	);
});

test('integers beyond 2^53 keep every digit', () => {
	assert.equal(toCanonical(9007199254740993n), '9007199254740993\n');
});

test('reals use ECMAScript shortest round-trip digits', () => {
	assert.equal(formatReal(1e21), '1e+21');
	assert.equal(formatReal(1e-7), '1e-7');
	assert.equal(formatReal(-0), '0.0');
	assert.equal(formatReal(0.1 + 0.2), '0.30000000000000004');
	assert.equal(toCanonical(sqlReal(Infinity)), '{"$real":"Infinity"}\n');
});

test('keys sort by code point, not by UTF-16 unit', () => {
	const keys = ['\u{1F600}', '�', 'b', 'B', 'a'].sort(compareCodePoints);
	assert.deepEqual(keys, ['B', 'a', 'b', '�', '\u{1F600}']);
});

test('empty containers and nested objects follow the layout rules', () => {
	assert.equal(toCanonical({ b: [], a: {}, c: { d: [1, [2, { z: 1, y: 2 }]] } }), '{\n\t"a": {},\n\t"b": [],\n\t"c": {\n\t\t"d": [\n\t\t\t1,\n\t\t\t[2,{"y":2,"z":1}]\n\t\t]\n\t}\n}\n');
});
