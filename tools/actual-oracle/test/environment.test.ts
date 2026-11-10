import assert from 'node:assert/strict';
import { existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { test } from 'node:test';

import { FIXTURES, runOracle, scratchDir } from './helpers.ts';

const SANDBOX_EXEC = '/usr/bin/sandbox-exec';
const DENY_NETWORK = '(version 1)(allow default)(deny network*)';

test('Actual works with every network call denied by the OS', { skip: !existsSync(SANDBOX_EXEC) && 'needs macOS sandbox-exec' }, t => {
	const out = join(scratchDir(t), 'small.zip');
	const probe = runOracle([], { wrapper: [SANDBOX_EXEC, '-p', DENY_NETWORK] });
	assert.equal(probe.status, 2, 'node itself must start inside the sandbox');

	const result = runOracle(['generate', 'small', '--out', out], { wrapper: [SANDBOX_EXEC, '-p', DENY_NETWORK] });
	assert.equal(result.status, 0, result.stderr);
	assert.ok(readFileSync(out).equals(readFileSync(join(FIXTURES, 'small.zip'))));

	const dump = runOracle(['dump', out], { wrapper: [SANDBOX_EXEC, '-p', DENY_NETWORK] });
	assert.equal(dump.status, 0, dump.stderr);
	assert.match(dump.stdout, /"computed"/);
});

test('a run leaves nothing behind in its temp dir, home or working directory', t => {
	const root = scratchDir(t);
	const [tmp, home, cwd, outDir] = ['tmp', 'home', 'cwd', 'out'].map(name => join(root, name));
	for (const dir of [tmp, home, cwd, outDir]) {
		mkdirSync(dir);
	}
	const env = { TMPDIR: tmp, HOME: home, XDG_CONFIG_HOME: join(home, '.config'), XDG_DATA_HOME: join(home, '.local/share') };
	const out = join(outDir, 'small.zip');

	for (const args of [['generate', 'small', '--out', out], ['dump', out, '--out', join(outDir, 'dump.json')]]) {
		const result = runOracle(args, { env, cwd });
		assert.equal(result.status, 0, result.stderr);
	}

	assert.deepEqual(readdirSync(tmp), []);
	assert.deepEqual(readdirSync(home), []);
	assert.deepEqual(readdirSync(cwd), []);
	assert.deepEqual(readdirSync(outDir).sort(), ['dump.json', 'small.zip']);
});
