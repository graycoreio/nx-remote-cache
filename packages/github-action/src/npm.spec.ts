import { createHash } from 'crypto';
import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';

import { checkIntegrity, installServer } from './npm';

const tarball = Buffer.from('a server package');
const integrity = `sha512-${createHash('sha512').update(tarball).digest('base64')}`;
const tarballUrl = 'https://registry.example/@graycore/nx-remote-cache-linux-x64-gnu/-/nx-remote-cache-linux-x64-gnu-0.4.2.tgz';

function registry(dist: Record<string, unknown> = { tarball: tarballUrl, integrity }, body: Buffer = tarball) {
    const requests: string[] = [];
    const fetchImpl = (async (url: string) => {
        requests.push(url);
        if (url === 'https://registry.example/@graycore%2fnx-remote-cache-linux-x64-gnu/0.4.2') {
            return new Response(JSON.stringify({ version: '0.4.2', dist }));
        }
        if (url === tarballUrl) return new Response(new Uint8Array(body));
        return new Response(JSON.stringify({ error: 'Not found' }), { status: 404 });
    }) as unknown as typeof fetch;
    return { requests, fetchImpl };
}

describe('installServer', () => {
    let dir: string;
    beforeEach(() => {
        dir = fs.mkdtempSync(path.join(os.tmpdir(), 'nx-remote-cache-npm-'));
    });
    afterEach(() => fs.rmSync(dir, { recursive: true, force: true }));

    // Writes the binary where a real tarball would put it.
    const extract = async (tgz: string, to: string) => {
        expect(fs.readFileSync(tgz)).toEqual(tarball);
        fs.mkdirSync(path.join(to, 'package', 'bin'), { recursive: true });
        fs.writeFileSync(path.join(to, 'package', 'bin', 'nx-remote-cache'), '#!/bin/sh\n');
    };
    const options = { version: '0.4.2', platform: 'linux' as const, arch: 'x64', dir: '' };

    it("installs the runner's platform package and checks it against the registry's integrity hash", async () => {
        const { requests, fetchImpl } = registry();
        const binary = await installServer({ ...options, dir }, { fetchImpl, registry: 'https://registry.example', extract });
        expect(binary).toBe(path.join(dir, 'package', 'bin', 'nx-remote-cache'));
        expect(fs.statSync(binary).mode & 0o111).toBeTruthy();
        expect(requests).toEqual(['https://registry.example/@graycore%2fnx-remote-cache-linux-x64-gnu/0.4.2', tarballUrl]);
    });

    it('refuses a tarball that does not match', async () => {
        const { fetchImpl } = registry(undefined, Buffer.from('something else'));
        await expect(installServer({ ...options, dir }, { fetchImpl, registry: 'https://registry.example', extract })).rejects.toThrow(
            /does not match its integrity hash/,
        );
    });

    it('explains a version that is not published', async () => {
        const { fetchImpl } = registry();
        await expect(
            installServer({ ...options, version: '9.9.9', dir }, { fetchImpl, registry: 'https://registry.example', extract }),
        ).rejects.toThrow('@graycore/nx-remote-cache-linux-x64-gnu@9.9.9 is not on https://registry.example: HTTP 404');
    });

    it('refuses a version without an integrity hash', async () => {
        const { fetchImpl } = registry({ tarball: tarballUrl });
        await expect(installServer({ ...options, dir }, { fetchImpl, registry: 'https://registry.example', extract })).rejects.toThrow(
            /no tarball or integrity hash/,
        );
    });

    it('knows no binary for an unsupported runner', async () => {
        await expect(installServer({ ...options, platform: 'win32', dir })).rejects.toThrow(/no server binary for win32\/x64/);
    });
});

describe('checkIntegrity', () => {
    it('accepts any matching sha512 among several hashes, and ignores other algorithms', () => {
        expect(() => checkIntegrity(tarball, `sha1-abc ${integrity}`, 'x')).not.toThrow();
        expect(() => checkIntegrity(tarball, 'sha1-abc', 'x')).toThrow(/no sha512 integrity hash/);
    });
});
