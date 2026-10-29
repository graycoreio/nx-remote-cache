import { execFile } from 'child_process';
import { createHash } from 'crypto';
import * as fs from 'fs';
import * as path from 'path';

import { platformPackage } from '../../nx-plugin/src/binary';
import { Fetch, requestJson } from './http';

export const NPM_REGISTRY = 'https://registry.npmjs.org';

export interface InstallOptions {
    /** A version, e.g. 0.4.2, or a dist-tag such as `latest`. */
    version: string;
    platform: NodeJS.Platform;
    arch: string;
    /** Where the package and binary go. */
    dir: string;
}

export interface InstallDeps {
    fetchImpl?: Fetch;
    registry?: string;
    extract?: (tarball: string, dir: string) => Promise<void>;
}

/**
 * Installs the server binary for this runner from its npm platform package, the one the Nx
 * plugin depends on, and returns its path. The tarball must match the integrity hash the registry
 * publishes for that version; npm never lets a published version be replaced.
 */
export async function installServer(o: InstallOptions, deps: InstallDeps = {}): Promise<string> {
    const { fetchImpl = fetch, registry = NPM_REGISTRY, extract = untar } = deps;
    const pkg = platformPackage(o.platform, o.arch);

    const metadataUrl = `${registry}/${pkg.replace('/', '%2f')}/${encodeURIComponent(o.version)}`;
    const metadata = await requestJson(metadataUrl, { headers: { accept: 'application/json' } }, fetchImpl).catch((e: Error) => {
        throw new Error(`${pkg}@${o.version} is not on ${registry}: ${e.message}`);
    });
    const dist = (metadata.dist ?? {}) as { tarball?: unknown; integrity?: unknown };
    if (typeof dist.tarball !== 'string' || typeof dist.integrity !== 'string') {
        throw new Error(`${pkg}@${o.version} has no tarball or integrity hash on ${registry}`);
    }

    let response: Response;
    try {
        response = await fetchImpl(dist.tarball);
    } catch (e) {
        throw new Error(`downloading ${dist.tarball} failed: ${e instanceof Error ? e.message : e}`);
    }
    if (!response.ok) {
        throw new Error(`downloading ${dist.tarball} failed: HTTP ${response.status}`);
    }
    const tarball = Buffer.from(await response.arrayBuffer());
    checkIntegrity(tarball, dist.integrity, `${pkg}@${metadata.version ?? o.version}`);

    fs.mkdirSync(o.dir, { recursive: true });
    const tarballPath = path.join(o.dir, 'server.tgz');
    fs.writeFileSync(tarballPath, tarball);
    await extract(tarballPath, o.dir);
    const binary = path.join(o.dir, 'package', 'bin', 'nx-remote-cache');
    fs.chmodSync(binary, 0o755);
    return binary;
}

/** Checks a Subresource Integrity string (`sha512-<base64>`, possibly several) against `data`. */
export function checkIntegrity(data: Buffer, integrity: string, what: string): void {
    const hashes = integrity.split(/\s+/).filter((h) => h.startsWith('sha512-'));
    if (hashes.length === 0) {
        throw new Error(`${what} has no sha512 integrity hash (${integrity}); refusing to run it`);
    }
    const actual = `sha512-${createHash('sha512').update(data).digest('base64')}`;
    if (!hashes.includes(actual)) {
        throw new Error(`${what} does not match its integrity hash (got ${actual}, expected ${hashes.join(' or ')}); refusing to run it`);
    }
}

function untar(tarball: string, dir: string): Promise<void> {
    return new Promise((resolve, reject) => {
        execFile('tar', ['-xzf', tarball, '-C', dir, 'package/bin/nx-remote-cache'], (error, _stdout, stderr) =>
            error ? reject(new Error(`extracting ${path.basename(tarball)} failed: ${stderr.trim() || error.message}`)) : resolve(),
        );
    });
}
