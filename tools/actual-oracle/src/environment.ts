import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

import { createRng, formatUuidV4 } from './rng.ts';

export const DEFAULT_TODAY = '2026-11-09';
export const DEFAULT_SEED = 1;

export interface Determinism {
	today: string;
	seed: number;
}

export interface ActualSession {
	api: any;
	internal: any;
	dataDir: string;
	today: string;
}

const primedIds: string[] = [];
const networkAttempts: string[] = [];
let installedWith: Determinism | null = null;

const installFixedClock = (today: string) => {
	const RealDate = globalThis.Date;
	const [year, month, day] = today.split('-').map(Number);
	const start = new RealDate(year, month - 1, day, 12, 0, 0, 0).getTime();
	let calls = 0;
	// Advancing one millisecond per thousand reads keeps CRDT timestamps strictly
	// increasing without letting a large generator run drift past midnight.
	const now = () => start + Math.floor(calls++ / 1000);
	globalThis.Date = new Proxy(RealDate, {
		construct: (target, args, newTarget) =>
			Reflect.construct(target, args.length === 0 ? [now()] : args, newTarget),
		apply: () => new RealDate(now()).toString(),
		get: (target, property, receiver) =>
			property === 'now' ? now : Reflect.get(target, property, receiver)
	});
};

const installSeededRandomness = (seed: number) => {
	const rng = createRng(seed);
	const randomUUID = () => primedIds.shift() ?? formatUuidV4(rng.fill(new Uint8Array(16)));
	const getRandomValues = <T extends ArrayBufferView | null>(array: T): T => {
		if (array) {
			rng.fill(new Uint8Array(array.buffer, array.byteOffset, array.byteLength));
		}
		return array;
	};
	Math.random = rng.next;
	Object.defineProperty(globalThis.crypto, 'randomUUID', { value: randomUUID, configurable: true, writable: true });
	Object.defineProperty(globalThis.crypto, 'getRandomValues', { value: getRandomValues, configurable: true, writable: true });
};

const installNetworkTripwire = () => {
	globalThis.fetch = async (input: unknown) => {
		networkAttempts.push(String(input instanceof Request ? input.url : input));
		throw new Error(`actual-oracle is offline: refused a request to ${String(input)}`);
	};
};

const installDeterminism = (determinism: Determinism) => {
	if (installedWith) {
		if (installedWith.today !== determinism.today || installedWith.seed !== determinism.seed) {
			throw new Error('Determinism settings can only be installed once per process');
		}
		return;
	}
	process.env.TZ = 'UTC';
	installFixedClock(determinism.today);
	installSeededRandomness(determinism.seed);
	installNetworkTripwire();
	console.log = console.error;
	console.info = console.error;
	console.warn = console.error;
	console.debug = () => {};
	installedWith = determinism;
};

export const primeIds = (...ids: string[]) => {
	primedIds.push(...ids);
};

export const takeUnusedPrimedIds = () => primedIds.splice(0);

export const withTempDir = async <T>(prefix: string, fn: (dir: string) => Promise<T>): Promise<T> => {
	const dir = mkdtempSync(join(tmpdir(), prefix));
	try {
		return await fn(dir);
	} finally {
		rmSync(dir, { recursive: true, force: true });
	}
};

export const withActual = async <T>(
	determinism: Determinism,
	fn: (session: ActualSession) => Promise<T>
): Promise<T> => {
	installDeterminism(determinism);
	const { default: api } = await import('@actual-app/api');
	const result = await withTempDir('actual-oracle-data-', async dataDir => {
		const internal = await api.init({ dataDir, verbose: false });
		try {
			return await fn({ api, internal, dataDir, today: determinism.today });
		} finally {
			// api.shutdown() runs a sync, and an offline sync reports success, which makes
			// Actual auto-post schedules and save prefs without awaiting either. Closing the
			// budget first leaves that listener nothing to act on.
			await internal.send('close-budget');
			await api.shutdown();
		}
	});
	if (networkAttempts.length > 0) {
		throw new Error(`Actual tried to reach the network: ${networkAttempts.join(', ')}`);
	}
	return result;
};
