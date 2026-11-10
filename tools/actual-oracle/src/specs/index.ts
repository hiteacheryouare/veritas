import { allFeatures } from './allFeatures.ts';
import type { FixtureSpec } from './builders.ts';
import { perf100k } from './perf100k.ts';
import { small } from './small.ts';

export const fixtureSpecs: Record<string, FixtureSpec> = {
	small,
	'all-features': allFeatures,
	'perf-100k': perf100k
};
