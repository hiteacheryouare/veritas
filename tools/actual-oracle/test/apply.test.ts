import assert from 'node:assert/strict';
import { existsSync, writeFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

import { nameId } from '../src/rng.ts';
import { FIXTURES, runOracle, runOracleOk, scratchDir } from './helpers.ts';

const small = (kind: string, name: string) => nameId(`small:${kind}:${name}`);
const groceries = small('category', 'groceries');
const dining = small('category', 'dining');
const checking = small('account', 'checking');
const lateDinner = nameId('apply-test:txn:late-dinner');

const applyAndDump = (t: Parameters<Parameters<typeof test>[1]>[0], ops: unknown[]) => {
	const dir = scratchDir(t);
	const opsPath = join(dir, 'ops.json');
	const out = join(dir, 'out.zip');
	writeFileSync(opsPath, JSON.stringify({ version: 1, ops }));
	runOracleOk(['apply', join(FIXTURES, 'small.zip'), opsPath, '--out', out]);
	return JSON.parse(runOracleOk(['dump', out]));
};

test('apply runs budget edits through Actual and the dump shows its numbers (R-BUD-1)', t => {
	const dump = applyAndDump(t, [
		{ op: 'setCarryover', month: '2026-10', category: groceries, flag: true },
		{ op: 'setBudgetAmount', month: '2026-10', category: groceries, amount: 10000 },
		{
			op: 'addTransactions',
			account: checking,
			transactions: [{ id: lateDinner, date: '2026-10-30', amount: -1000, payee_name: 'Noodle Bar', category: dining }]
		},
		{ op: 'holdForNextMonth', month: '2026-10', amount: 50000 }
	]);
	const october = dump.computed.months['2026-10'];
	const november = dump.computed.months['2026-11'];

	assert.equal(october.totalBudgeted, -174000);
	assert.equal(october.forNextMonth, 50000);
	assert.equal(october.toBudget, 847343 - 657 - 174000 - 50000);
	assert.deepEqual(october.categories[groceries], { balance: -2657, budgeted: 10000, carryover: true, spent: -12657 });
	assert.deepEqual(october.categories[dining], { balance: -430, budgeted: 5000, carryover: false, spent: -5810 });

	assert.equal(november.categories[groceries].balance, -2657, 'carryover keeps the overspending in the envelope');
	assert.equal(november.categories[dining].balance, 0, 'overspending without carryover leaves the envelope');
	assert.equal(november.lastMonthOverspent, -430);

	assert.equal(dump.computed.accounts[checking].balance, 622330 - 1000);
	const added = dump.tables.transactions.rows.find((row: { id: string }) => row.id === lateDinner);
	assert.equal(added.amount, -1000);
	assert.equal(added.date, 20261030);
});

test('apply refuses an unknown op and names it', t => {
	const dir = scratchDir(t);
	const opsPath = join(dir, 'ops.json');
	writeFileSync(opsPath, JSON.stringify({ version: 1, ops: [{ op: 'launchRocket' }] }));
	const result = runOracle(['apply', join(FIXTURES, 'small.zip'), opsPath, '--out', join(dir, 'out.zip')]);
	assert.notEqual(result.status, 0);
	assert.match(result.stderr, /Op #0 has unknown type "launchRocket"/);
});

test('apply refuses a create whose id is already taken, instead of overwriting that row', t => {
	const dir = scratchDir(t);
	const opsPath = join(dir, 'ops.json');
	const out = join(dir, 'out.zip');
	const reused = { op: 'createCategory', id: groceries, name: 'Impostor', group: small('group', 'bills') };
	writeFileSync(opsPath, JSON.stringify({ version: 1, ops: [reused] }));
	const result = runOracle(['apply', join(FIXTURES, 'small.zip'), opsPath, '--out', out]);
	assert.notEqual(result.status, 0);
	assert.match(result.stderr, /Op #0 \(createCategory\) failed: A categories row with id [0-9a-f-]+ already exists/);
	assert.equal(existsSync(out), false);
});
