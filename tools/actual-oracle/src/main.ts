import { closeSync, mkdirSync, openSync, readFileSync, writeFileSync, writeSync } from 'node:fs';
import { dirname, resolve } from 'node:path';
import { parseArgs } from 'node:util';

import { writeCanonical } from './canonicalJson.ts';
import type { CanonicalValue } from './canonicalJson.ts';
import { computedDump, rawDump } from './dump.ts';
import { DEFAULT_SEED, DEFAULT_TODAY, withActual } from './environment.ts';
import type { Determinism } from './environment.ts';
import { applyOps, parseOpFile } from './ops.ts';
import { reportRows } from './reportRows.ts';
import { fixtureSpecs } from './specs/index.ts';

const USAGE = `Usage:
  node src/main.ts dump <zip> [--out <file>] [--raw-only]
  node src/main.ts apply <zip> <ops.json> --out <zip>
  node src/main.ts generate <${Object.keys(fixtureSpecs).join('|')}> [--out <zip>]
  node src/main.ts report-rows <zip> <requests.json> [--out <file>]

Shared options:
  --today <YYYY-MM-DD>  the date Actual sees as today (default ${DEFAULT_TODAY})
  --seed <n>            seed for every id and random value Actual generates (default ${DEFAULT_SEED})`;

const FIXTURE_DIR = resolve(import.meta.dirname, '../../../fixtures/synthetic');
const FLUSH_BYTES = 1 << 20;

const writeCanonicalTo = (value: CanonicalValue, out: string | undefined) => {
	const fd = out ? openSync(out, 'w') : 1;
	let pending: string[] = [];
	let pendingLength = 0;
	const flush = () => {
		writeSync(fd, pending.join(''));
		pending = [];
		pendingLength = 0;
	};
	writeCanonical(value, chunk => {
		pending.push(chunk);
		pendingLength += chunk.length;
		if (pendingLength >= FLUSH_BYTES) {
			flush();
		}
	});
	pending.push('\n');
	flush();
	if (out) {
		closeSync(fd);
	}
};

const writeZip = (path: string, bytes: Uint8Array) => {
	mkdirSync(dirname(path), { recursive: true });
	writeFileSync(path, bytes);
};

const dump = async (determinism: Determinism, zipPath: string, out: string | undefined, rawOnly: boolean) => {
	const zipBytes = readFileSync(zipPath);
	const raw = await rawDump(zipBytes);
	const computed = rawOnly
		? undefined
		: await withActual(determinism, async session => {
				await session.api.importBudget(zipBytes);
				return computedDump(session);
			});
	writeCanonicalTo(computed ? { ...raw, computed } : raw, out);
};

const apply = (determinism: Determinism, zipPath: string, opsPath: string, out: string) =>
	withActual(determinism, async session => {
		const ops = parseOpFile(readFileSync(opsPath, 'utf8'));
		await session.api.importBudget(readFileSync(zipPath));
		await applyOps(session, ops);
		writeZip(out, await session.api.exportBudget());
	});

const generate = (determinism: Determinism, specName: string, out: string) => {
	const spec = fixtureSpecs[specName];
	if (!spec) {
		throw new Error(`Unknown fixture spec "${specName}". Known specs: ${Object.keys(fixtureSpecs).join(', ')}`);
	}
	return withActual(determinism, async session => {
		await session.api.runImport(spec.budgetName, async () => {});
		await applyOps(session, spec.build());
		writeZip(out, await session.api.exportBudget());
	});
};

const main = async () => {
	const { positionals, values } = parseArgs({
		allowPositionals: true,
		options: {
			out: { type: 'string' },
			'raw-only': { type: 'boolean', default: false },
			today: { type: 'string', default: DEFAULT_TODAY },
			seed: { type: 'string', default: String(DEFAULT_SEED) },
			help: { type: 'boolean', default: false }
		}
	});
	const [command, ...args] = positionals;
	if (values.help || !command) {
		console.error(USAGE);
		process.exitCode = values.help ? 0 : 2;
		return;
	}
	if (!/^\d{4}-\d{2}-\d{2}$/.test(values.today)) {
		throw new Error(`--today must be YYYY-MM-DD, got "${values.today}"`);
	}
	const determinism = { today: values.today, seed: Number(values.seed) };

	if (command === 'dump' && args.length === 1) {
		await dump(determinism, args[0], values.out, values['raw-only']);
	} else if (command === 'apply' && args.length === 2 && values.out) {
		await apply(determinism, args[0], args[1], values.out);
	} else if (command === 'report-rows' && args.length === 2) {
		const requests = JSON.parse(readFileSync(args[1], 'utf8'));
		const rows = await withActual(determinism, async session => {
			await session.api.importBudget(readFileSync(args[0]));
			return reportRows(session, requests);
		});
		writeCanonicalTo(rows, values.out);
	} else if (command === 'generate' && args.length === 1) {
		await generate(determinism, args[0], values.out ?? resolve(FIXTURE_DIR, `${args[0]}.zip`));
	} else {
		console.error(USAGE);
		process.exitCode = 2;
	}
};

main().catch(error => {
	console.error(error instanceof Error ? error.stack : error);
	process.exitCode = 1;
});
