// Every theme color is a CSS variable named after its key (component-library/src/theme.ts).
export const theme = new Proxy({} as Record<string, string>, {
	get: (_, name) => `var(--color-${String(name)})`
});
