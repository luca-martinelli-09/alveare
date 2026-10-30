// Test nel browser contro uno stack vero (quello del test di integrazione con KEEP=1):
// E2E_CONFIG punta al file con URL, email, password e segreto TOTP dell'admin.
import { defineConfig, devices } from '@playwright/test';

export default defineConfig({
	testDir: 'e2e',
	timeout: 180_000,
	fullyParallel: false,
	workers: 1,
	reporter: 'list',
	use: { trace: 'retain-on-failure', locale: 'it-IT' },
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }]
});
