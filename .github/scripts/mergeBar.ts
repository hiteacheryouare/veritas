export const ROLES = ['reviewer', 'cowboy', 'fred', 'architect'] as const;
export type Role = typeof ROLES[number];

export interface Signoff {
	role: Role;
	verdict: 'approve' | 'reject';
	sha: string;
	requirements: string[];
}

export interface PullRequest {
	baseRef: string;
	headRef: string;
	headSha: string;
	reviewedSha: string;
	fromFork: boolean;
	body: string;
	labels: string[];
}

export interface Verdict {
	state: 'success' | 'pending';
	description: string;
}

const SIGNOFF = /^merge-bar:\s*(reviewer|cowboy|fred|architect)\s+(approve|reject)\s+([0-9a-f]{7,40})\b(.*)$/gim;
const REQUIREMENT = /\bR-[A-Z]+-\d+\b/g;
const TRUSTED = new Set(['OWNER', 'MEMBER', 'COLLABORATOR']);

const prose = (markdown: string) => markdown.replace(/<!--[\s\S]*?-->/g, '').replace(/^(`{3,}|~{3,})[\s\S]*?^\1/gm, '');

export const parseSignoffs = (text: string): Signoff[] =>
	[...prose(text).matchAll(SIGNOFF)].map(([, role, verdict, sha, rest]) => ({
		role: role.toLowerCase() as Role,
		verdict: verdict.toLowerCase() as Signoff['verdict'],
		sha: sha.toLowerCase(),
		requirements: rest.match(REQUIREMENT) ?? []
	}));

export const trustedSignoffs = (posts: { body: string | null; authorAssociation: string }[]): Signoff[] =>
	posts.filter((post) => TRUSTED.has(post.authorAssociation)).flatMap((post) => parseSignoffs(post.body ?? ''));

export const claimedRequirements = (body: string): string[] => {
	const line = prose(body).split(/\r?\n/).find((text) => /^\s*claims:/i.test(text));
	return [...new Set(line?.match(REQUIREMENT) ?? [])].sort();
};

export interface Commit {
	sha: string;
	parents: string[];
}

export const reviewedCommit = (headSha: string, commits: Commit[], isCleanSync: (commit: Commit) => boolean): string => {
	const bySha = new Map(commits.map((commit) => [commit.sha, commit]));
	let current = bySha.get(headSha);
	if (!current) {
		return headSha;
	}
	while (current.parents.length === 2 && isCleanSync(current)) {
		const previous = bySha.get(current.parents[0]);
		if (!previous) {
			return current.sha;
		}
		current = previous;
	}
	return current.sha;
};

export const evaluate = (pr: PullRequest, signoffs: Signoff[]): Verdict => {
	if (pr.baseRef === 'main') {
		return pr.headRef === 'dev' && !pr.fromFork
			? { state: 'success', description: 'Gate PR from dev: Ryan merges these himself' }
			: { state: 'pending', description: 'Only dev merges into main' };
	}
	const latest = new Map<Role, Signoff>();
	for (const signoff of signoffs) {
		if (pr.headSha.startsWith(signoff.sha) || pr.reviewedSha.startsWith(signoff.sha)) {
			latest.set(signoff.role, signoff);
		}
	}
	const approved = (role: Role) => latest.get(role)?.verdict === 'approve';
	const claims = claimedRequirements(pr.body);
	const missing: string[] = [];

	if (!approved('reviewer')) {
		missing.push('reviewer');
	}
	if (claims.length > 0) {
		const covered = new Set(approved('cowboy') ? latest.get('cowboy')?.requirements : []);
		const uncovered = claims.filter((id) => !covered.has(id));
		if (uncovered.length > 0) {
			missing.push(`cowboy (${uncovered.join(', ')})`);
		}
	}
	if (pr.labels.includes('finance') && !approved('fred')) {
		missing.push('fred');
	}
	const architectAuthored = pr.labels.includes('architect-authored');
	if (pr.labels.includes('contract') && !architectAuthored && !approved('architect')) {
		missing.push('architect');
	}

	if (missing.length === 0) {
		return { state: 'success', description: `Signed off on ${pr.headSha.slice(0, 7)}` };
	}
	return { state: 'pending', description: `Needs ${missing.join(', ')} on ${pr.headSha.slice(0, 7)}`.slice(0, 140) };
};
