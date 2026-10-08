import { spawnSync } from 'node:child_process';
import { cpSync, mkdirSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
for (const [command, args] of [
    ['cargo', ['build', '-p', 'arpeg-wasm', '--no-default-features', '--target', 'wasm32-unknown-unknown', '--release', '--locked']],
    ['wasm-bindgen', ['--target', 'web', '--out-dir', 'dist/wasm',
        join(process.env.CARGO_TARGET_DIR ?? 'target', 'wasm32-unknown-unknown/release/arpeg_wasm.wasm')]],
]) {
    const result = spawnSync(command, args, { cwd: root, stdio: 'inherit' });
    if (result.error) { console.error(result.error.message); process.exit(1); }
    if (result.status !== 0) process.exit(result.status ?? 1);
}
const output = join(root, 'dist/web');
mkdirSync(output, { recursive: true });
for (const file of ['index.html', 'style.css', 'app.mjs', 'music.mjs', 'performance.mjs', 'synth.mjs']) {
    cpSync(join(root, 'web', file), join(output, file));
}
cpSync(join(root, 'dist/wasm'), join(output, 'wasm'), { recursive: true });
console.log('Browser instrument built in dist/web. Serve that directory over HTTP.');
