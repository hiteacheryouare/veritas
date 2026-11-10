import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

import { FIXTURES, runOracleOk, scratchDir } from './helpers.ts';

const CLOCK_NODE = /-[0-9a-f]{16}\\"/;

for (const fixture of ['small', 'all-features']) {
	test(`dumping ${fixture} twice gives byte-identical output`, () => {
		const zip = join(FIXTURES, `${fixture}.zip`);
		const first = runOracleOk(['dump', zip]);
		const second = runOracleOk(['dump', zip]);
		assert.ok(first.length > 1000);
		assert.ok(first === second, 'dumps differ');
	});

	test(`${fixture} regenerates byte for byte from its spec`, t => {
		const out = join(scratchDir(t), `${fixture}.zip`);
		runOracleOk(['generate', fixture, '--out', out]);
		assert.ok(readFileSync(out).equals(readFileSync(join(FIXTURES, `${fixture}.zip`))), 'regenerated zip differs');
	});

	test(`Actual's own import then export keeps every ${fixture} table except the clock's node id (R-IO-3)`, t => {
		const dir = scratchDir(t);
		const opsPath = join(dir, 'no-ops.json');
		const exported = join(dir, 'exported.zip');
		writeFileSync(opsPath, JSON.stringify({ version: 1, ops: [] }));
		runOracleOk(['apply', join(FIXTURES, `${fixture}.zip`), opsPath, '--out', exported]);

		const before = runOracleOk(['dump', join(FIXTURES, `${fixture}.zip`), '--raw-only']).split('\n');
		const after = runOracleOk(['dump', exported, '--raw-only']).split('\n');
		assert.equal(after.length, before.length);
		const differing = before.flatMap((line, index) => (line === after[index] ? [] : [[line, after[index]]]));
		assert.equal(differing.length, 1, `expected only messages_clock to differ, got ${differing.length} lines`);
		const [[original, roundTripped]] = differing;
		assert.match(original, /"clock":/);
		assert.equal(original.replace(CLOCK_NODE, ''), roundTripped.replace(CLOCK_NODE, ''));
	});
}
