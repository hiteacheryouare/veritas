import { spawnSync } from 'node:child_process';
import type { SpawnSyncReturns } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import type { TestContext } from 'node:test';

export const MAIN = resolve(import.meta.dirname, '../src/main.ts');
export const FIXTURES = resolve(import.meta.dirname, '../../../fixtures/synthetic');

export interface RunOptions {
	env?: NodeJS.ProcessEnv;
	cwd?: string;
	wrapper?: string[];
}

export const runOracle = (args: string[], { env, cwd, wrapper = [] }: RunOptions = {}): SpawnSyncReturns<string> => {
	const command = [...wrapper, process.execPath, MAIN, ...args];
	return spawnSync(command[0], command.slice(1), {
		cwd,
		encoding: 'utf8',
		env: { ...process.env, ...env },
		maxBuffer: 1 << 30
	});
};

export const runOracleOk = (args: string[], options?: RunOptions): string => {
	const result = runOracle(args, options);
	if (result.status !== 0) {
		throw new Error(`oracle ${args.join(' ')} exited ${result.status}:\n${result.stderr}`);
	}
	return result.stdout;
};

export const scratchDir = (t: TestContext, prefix = 'actual-oracle-test-'): string => {
	const dir = mkdtempSync(join(tmpdir(), prefix));
	t.after(() => rmSync(dir, { recursive: true, force: true }));
	return dir;
};
