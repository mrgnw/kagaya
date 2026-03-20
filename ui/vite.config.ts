import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig } from 'vite';

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
	plugins: [sveltekit()],
	clearScreen: false,
	server: {
		port: 13369,
		strictPort: true,
		host: '0.0.0.0',
		hmr: host
			? { protocol: 'ws', host, port: 1421 }
			: undefined,
		allowedHosts: ['.xcc.es', '.skate-in.ts.net'],
		proxy: {
			'/api': 'http://localhost:13370',
			'/ws': { target: 'ws://localhost:13370', ws: true },
		},
	},
});
