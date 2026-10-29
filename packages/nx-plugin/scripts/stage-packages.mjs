#!/usr/bin/env node
// Builds the npm packages for a release from its archives:
//   node scripts/stage-packages.mjs <archives-dir> <out-dir>
// The archives are named with the version archive-release.sh gives them, worked out the same way:
// the tag's without its `v` on a tag run, `dev` in other CI runs, or `local` outside CI. Every
// package gets the plugin's version, which release-please bumps. Every platform's archive is
// required, except for `local`, which stages the platforms it has archives for, e.g. only this
// machine's.
// → <out-dir>/<one dir per platform package>, then <out-dir>/nx-remote-cache (the plugin, built),
//   each named after its package without the scope (e.g. nx-remote-cache-linux-x64-gnu), so it can
//   be copied straight into node_modules/@graycore, and ready for `npm publish`. Platform packages are listed first in <out-dir>/order.txt,
//   because the plugin's optional dependencies must exist before it is published.
import { execFileSync } from 'node:child_process';
import * as fs from 'node:fs';
import { createRequire } from 'node:module';
import * as path from 'node:path';

const [archives, out] = process.argv.slice(2);
if (!archives || !out) {
    console.error('usage: stage-packages.mjs <archives-dir> <out-dir>');
    process.exit(2);
}

// The version in the archive names. Keep in step with archive-release.sh.
const getArchiveVersion = () => {
    // A tag run: the tag without its `v`, e.g. v0.5.0 → 0.5.0.
    if (process.env.GITHUB_REF_TYPE === 'tag') {
        return process.env.GITHUB_REF_NAME.replace(/^v/, '');
    }
    // Any other CI run.
    if (process.env.CI) {
        return 'dev';
    }
    // A developer's machine.
    return 'local';
};
const archiveVersion = getArchiveVersion();

// Release target → npm package suffix and the platform fields npm installs it for.
const PLATFORMS = {
    'x86_64-unknown-linux-gnu': { suffix: 'linux-x64-gnu', os: ['linux'], cpu: ['x64'], libc: ['glibc'] },
    'aarch64-unknown-linux-gnu': { suffix: 'linux-arm64-gnu', os: ['linux'], cpu: ['arm64'], libc: ['glibc'] },
    'universal-apple-darwin': { suffix: 'darwin-universal', os: ['darwin'], cpu: ['x64', 'arm64'] },
};

const archiveOf = (target) => path.join(archives, `nx-remote-cache-${archiveVersion}-${target}.tar.gz`);
const targets = Object.keys(PLATFORMS);
const missing = targets.filter((target) => !fs.existsSync(archiveOf(target)));
const staged = targets.filter((target) => !missing.includes(target));
if (archiveVersion === 'local' ? !staged.length : missing.length) {
    console.error(`Missing release archives:\n${missing.map((t) => `  ${archiveOf(t)}`).join('\n')}`);
    console.error('\nTo build the archive for this machine, run: npx nx run server:archive');
    process.exit(1);
}
for (const target of missing) {
    console.warn(`Skipping ${target}: no archive at ${archiveOf(target)}`);
}

const pluginDir = path.resolve(import.meta.dirname, '..');
const plugin = JSON.parse(fs.readFileSync(path.join(pluginDir, 'package.json'), 'utf8'));
const { version } = plugin;
// The plugin, compiled by `nx run nx-plugin:build`; it also tells which package it looks for on
// each platform.
if (!fs.existsSync(path.join(pluginDir, 'dist/binary.js'))) {
    console.error('The plugin is not built; run `npx nx run nx-plugin:build` first.');
    process.exit(1);
}
const { platformPackage } = createRequire(import.meta.url)(path.join(pluginDir, 'dist/binary.js'));
fs.rmSync(out, { recursive: true, force: true });
const order = [];
const optionalDependencies = {};

for (const [target, p] of Object.entries(PLATFORMS)) {
    const name = `${plugin.name}-${p.suffix}`;
    for (const os of p.os) {
        for (const cpu of p.cpu) {
            if (platformPackage(os, cpu) !== name) {
                throw new Error(`the plugin looks for ${platformPackage(os, cpu)} on ${os}/${cpu}, not ${name}`);
            }
        }
    }
    if (!staged.includes(target)) {
        continue;
    }
    optionalDependencies[name] = version;
    const archive = archiveOf(target);
    const dir = path.join(out, path.basename(name));
    fs.mkdirSync(path.join(dir, 'bin'), { recursive: true });
    execFileSync('tar', ['-xzf', archive, '-C', path.join(dir, 'bin'), 'nx-remote-cache']);
    execFileSync('tar', ['-xzf', archive, '-C', dir, 'LICENSE']);
    const { libc, suffix, ...platform } = p;
    writeJson(path.join(dir, 'package.json'), {
        name,
        version,
        description: `The nx-remote-cache server binary for ${suffix}, used by ${plugin.name}.`,
        license: plugin.license,
        repository: plugin.repository,
        ...platform,
        ...(libc ? { libc } : {}),
        files: ['bin', 'LICENSE'],
        // Yarn PnP: keep the binary on disk, where it can be run.
        preferUnplugged: true,
    });
    order.push(dir);
}

// The plugin, depending on the platform packages: npm installs the one matching each machine.
const dir = path.join(out, path.basename(plugin.name));
fs.mkdirSync(dir, { recursive: true });
fs.cpSync(path.join(pluginDir, 'dist'), path.join(dir, 'dist'), {
    recursive: true,
    filter: (src) => !/\.test\.(js|d\.ts)$/.test(src),
});
// tsc writes plain files. npm marks bin scripts executable when it installs a package, but not
// when it links a directory, e.g. this one through `file:`, after the link was made.
for (const bin of Object.values(plugin.bin ?? {})) {
    fs.chmodSync(path.join(dir, bin), 0o755);
}
fs.copyFileSync(path.resolve(pluginDir, '../../LICENSE'), path.join(dir, 'LICENSE'));
fs.copyFileSync(path.join(pluginDir, 'README.md'), path.join(dir, 'README.md'));
const { scripts, devDependencies, ...manifest } = plugin;
writeJson(path.join(dir, 'package.json'), {
    ...manifest,
    version,
    files: ['dist', 'LICENSE', 'README.md'],
    optionalDependencies,
});
order.push(dir);

fs.writeFileSync(path.join(out, 'order.txt'), order.join('\n') + '\n');
console.log(order.join('\n'));

function writeJson(file, value) {
    fs.writeFileSync(file, JSON.stringify(value, null, 2) + '\n');
}
