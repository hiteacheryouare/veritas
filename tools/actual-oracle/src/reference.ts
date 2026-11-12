import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync } from 'node:fs';
import { registerHooks } from 'node:module';
import { join, resolve } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Actual's bundle tree-shakes evalArithmetic away, so these functions run
// from the pinned, unmodified source checkout instead (docs/oracle.md).
const ROOT = resolve(import.meta.dirname, '../../..');
const ORACLE_PACKAGE = pathToFileURL(resolve(import.meta.dirname, '../package.json')).href;
const FIX = 'run `cargo xtask reference` from the repository root';

export interface Pin {
	tag: string;
	commit: string;
}

export interface ActualArithmetic {
	pin: Pin;
	evalArithmetic: (expression: string, defaultValue?: number | null) => number | null;
	currencyToAmount: (text: string) => number | null;
	amountToInteger: (amount: number, decimalPlaces?: number) => number;
	setNumberFormat: (config: { format: string; hideFraction: boolean }) => void;
	getNumberFormat: (config: { format?: string; hideFraction?: boolean; decimalPlaces?: number }) => {
		formatter: { format: (value: number) => string };
	};
}

export const referenceDir = () => process.env.ACTUAL_REFERENCE_DIR ?? join(ROOT, 'reference/actual');

const readPin = (): Pin => {
	const dir = join(ROOT, 'crates/veritas-interop/manifest');
	const [file, ...others] = readdirSync(dir).filter(name => name.endsWith('.json'));
	if (!file || others.length > 0) {
		throw new Error(`Expected exactly one migration manifest in ${dir}`);
	}
	const manifest = JSON.parse(readFileSync(join(dir, file), 'utf8'));
	return { tag: manifest.actualTag, commit: manifest.actualCommit };
};

export const verifiedReference = (dir = referenceDir()): Pin => {
	const pin = readPin();
	if (!existsSync(join(dir, 'packages/loot-core/src/shared/arithmetic.ts'))) {
		throw new Error(`Actual's source isn't at ${dir}: ${FIX}`);
	}
	let head: string;
	try {
		head = execFileSync('git', ['-C', dir, 'rev-parse', 'HEAD'], { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
	} catch {
		throw new Error(`${dir} isn't a git checkout of Actual: ${FIX}`);
	}
	if (head !== pin.commit) {
		throw new Error(`${dir} is at ${head}, but the manifest pins ${pin.tag} (${pin.commit}): delete it and ${FIX}`);
	}
	return pin;
};

let hooksFor: string | null = null;

const installHooks = (dir: string) => {
	if (hooksFor === dir) {
		return;
	}
	hooksFor = dir;
	const sourceUrl = pathToFileURL(dir).href;
	registerHooks({
		resolve: (specifier, context, nextResolve) => {
			const fromSource = context.parentURL?.startsWith(sourceUrl) ?? false;
			if (fromSource && (specifier === 'date-fns' || specifier.startsWith('date-fns/'))) {
				return nextResolve(specifier, { ...context, parentURL: ORACLE_PACKAGE });
			}
			if (fromSource && specifier.startsWith('.') && !specifier.endsWith('.ts')) {
				const candidate = new URL(`${specifier}.ts`, context.parentURL);
				if (existsSync(fileURLToPath(candidate))) {
					return { url: candidate.href, shortCircuit: true };
				}
			}
			return nextResolve(specifier, context);
		},
		load: (url, context, nextLoad) =>
			url.startsWith(sourceUrl) && url.endsWith('.ts')
				? nextLoad(url, { ...context, format: 'module-typescript' })
				: nextLoad(url, context)
	});
};

export const loadArithmetic = async (dir = referenceDir()): Promise<ActualArithmetic> => {
	const pin = verifiedReference(dir);
	installHooks(dir);
	const shared = join(dir, 'packages/loot-core/src/shared');
	const arithmetic = await import(pathToFileURL(join(shared, 'arithmetic.ts')).href);
	const util = await import(pathToFileURL(join(shared, 'util.ts')).href);
	return {
		pin,
		evalArithmetic: arithmetic.evalArithmetic,
		currencyToAmount: util.currencyToAmount,
		amountToInteger: util.amountToInteger,
		setNumberFormat: util.setNumberFormat,
		getNumberFormat: util.getNumberFormat
	};
};
