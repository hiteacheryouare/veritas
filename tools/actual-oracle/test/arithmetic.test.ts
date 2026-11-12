import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { join, resolve } from 'node:path';
import { test } from 'node:test';

import { referenceDir } from '../src/reference.ts';
import { runOracle, runOracleOk, scratchDir } from './helpers.ts';

const ENGINE_VECTORS = resolve(import.meta.dirname, '../../../crates/veritas-engine/tests/fixtures/actual-arithmetic-vectors.json');
const missingReference = !existsSync(referenceDir());
if (missingReference && process.env.VERITAS_REQUIRE_ORACLE) {
	throw new Error('VERITAS_REQUIRE_ORACLE is set but reference/actual is missing: run `cargo xtask reference`');
}
const noReference = missingReference && 'needs reference/actual: run `cargo xtask reference`';

test('arithmetic-vectors reproduces the engine fixture from the pinned source (R-BUD-3)', { skip: noReference }, t => {
	const out = join(scratchDir(t), 'vectors.json');
	runOracleOk(['arithmetic-vectors', '--out', out]);
	const committed = readFileSync(ENGINE_VECTORS, 'utf8').replace(/\r\n/g, '\n');
	assert.equal(readFileSync(out, 'utf8'), committed);
});

test('eval runs each case under its own number format', { skip: noReference }, t => {
	const cases = join(scratchDir(t), 'cases.json');
	writeFileSync(
		cases,
		JSON.stringify([
			{ format: 'comma-dot', expression: '1,234.56+10' },
			{ format: 'dot-comma', expression: '1.234,56+10' },
			{ format: 'comma-dot', expression: '2^3^2' },
			{ format: 'comma-dot', expression: '1/0' },
			{ format: 'comma-dot', expression: '(5' }
		])
	);
	const results = JSON.parse(runOracleOk(['eval', cases])).map((r: { result: string | null }) => r.result);
	assert.deepEqual(results, ['1244.56', '1244.56', '512', 'Infinity', null]);
});

test('eval refuses a missing or unpinned reference and says how to fix it', t => {
	const dir = scratchDir(t);
	const cases = join(dir, 'cases.json');
	writeFileSync(cases, '[]');
	const missing = runOracle(['eval', cases], { env: { ACTUAL_REFERENCE_DIR: join(dir, 'nowhere') } });
	assert.notEqual(missing.status, 0);
	assert.match(missing.stderr, /isn't at .*run `cargo xtask reference`/);

	const wrong = join(dir, 'wrong');
	mkdirSync(join(wrong, 'packages/loot-core/src/shared'), { recursive: true });
	writeFileSync(join(wrong, 'packages/loot-core/src/shared/arithmetic.ts'), 'export {};\n');
	const git = (...args: string[]) =>
		execFileSync('git', ['-C', wrong, '-c', 'user.name=oracle', '-c', 'user.email=oracle@example.invalid', ...args], { stdio: 'ignore' });
	git('init', '--quiet');
	git('add', '.');
	git('commit', '--quiet', '-m', 'not Actual');
	const unpinned = runOracle(['eval', cases], { env: { ACTUAL_REFERENCE_DIR: wrong } });
	assert.notEqual(unpinned.status, 0);
	assert.match(unpinned.stderr, /but the manifest pins v26\.11\.0 .*run `cargo xtask reference`/);
});
