import assert from 'node:assert/strict';
import { readFileSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

import { nameId } from '../src/rng.ts';
import { FIXTURES, runOracle, runOracleOk, scratchDir } from './helpers.ts';

const groceries = nameId('small:category:groceries');
const dining = nameId('small:category:dining');

const replay = (t: Parameters<Parameters<typeof test>[1]>[0], sessions: unknown[][]) => {
	const dir = scratchDir(t);
	const input = join(dir, 'sessions.json');
	const out = join(dir, 'results.json');
	writeFileSync(input, JSON.stringify({ version: 1, sessions }));
	runOracleOk(['replay', join(FIXTURES, 'small.zip'), input, '--out', out]);
	return JSON.parse(readFileSync(out, 'utf8'));
};

const move = { op: 'budgetAction', action: 'transfer-category', args: { month: '2026-10', amount: 2000, from: groceries, to: dining, currencyCode: '' } };

test('replay starts every session from a fresh import of the zip (R-BUD-4)', t => {
	const [first, second, untouched] = replay(t, [[move], [move], []]);
	assert.equal(first.error, null);
	assert.deepEqual(first, second, 'the second session does not see the first one');
	assert.equal(first.months['2026-10'].categories[dining].budgeted, 7000);
	assert.equal(untouched.months['2026-10'].categories[dining].budgeted, 5000);
	assert.deepEqual(first.budgetNotes, [
		{ id: 'budget-2026-10', note: '- Reassigned 20.00 from Groceries → Dining Out on November 09' }
	]);
	assert.deepEqual(untouched.budgetNotes, []);
});

test('budgetAction runs the handler the API wraps with validation', t => {
	const [held] = replay(t, [[{ op: 'budgetAction', action: 'hold-for-next-month', args: { month: '2026-10', amount: -5 } }]]);
	assert.equal(held.error, null, 'the API would refuse a non-positive hold; the handler clamps it');
	assert.deepEqual(held.zeroBudgetMonths, [{ id: '2026-10', buffered: 0 }]);
});

test('a session whose op fails records the error and later sessions still run', t => {
	const [failed, fine] = replay(t, [[{ op: 'budgetAction', action: 'launch-rocket', args: {} }], [move]]);
	assert.match(failed.error, /"launch-rocket" isn't a budget action/);
	assert.equal(fine.error, null);
});

test('replay refuses a malformed sessions file', t => {
	const dir = scratchDir(t);
	const input = join(dir, 'sessions.json');
	writeFileSync(input, JSON.stringify({ sessions: [] }));
	const result = runOracle(['replay', join(FIXTURES, 'small.zip'), input, '--out', join(dir, 'out.json')]);
	assert.notEqual(result.status, 0);
	assert.match(result.stderr, /A sessions file must look like/);
});
