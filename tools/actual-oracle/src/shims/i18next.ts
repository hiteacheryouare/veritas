// Report modules translate their labels at import time; the oracle reads them
// as their English keys, with any {{name}} filled in.
export const t = (key: string, params?: Record<string, unknown>) =>
	params ? key.replace(/\{\{(\w+)\}\}/g, (whole, name) => (name in params ? String(params[name]) : whole)) : key;

export default { t };
