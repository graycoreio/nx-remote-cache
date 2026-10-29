import { strict as assert } from 'assert';
import { describe, it } from 'node:test';

import { platformPackage, resolveBinary } from './binary';

describe('resolveBinary', () => {
    it("resolves the platform package's binary", () => {
        const resolve = (id: string) => `/node_modules/${id}`;
        assert.equal(resolveBinary({}, resolve, 'linux', 'x64'), '/node_modules/@graycoreio/nx-remote-cache-linux-x64-gnu/bin/nx-remote-cache');
        assert.equal(platformPackage('linux', 'arm64'), '@graycoreio/nx-remote-cache-linux-arm64-gnu');
        assert.equal(platformPackage('darwin', 'x64'), '@graycoreio/nx-remote-cache-darwin-universal');
    });

    it('prefers NX_REMOTE_CACHE_BINARY', () => {
        assert.equal(resolveBinary({ NX_REMOTE_CACHE_BINARY: '/build/nx-remote-cache' }, () => assert.fail()), '/build/nx-remote-cache');
    });

    it('explains a missing platform package or an unsupported platform', () => {
        const missing = () => {
            throw new Error('MODULE_NOT_FOUND');
        };
        assert.throws(() => resolveBinary({}, missing, 'linux', 'x64'), /is not installed/);
        assert.throws(() => platformPackage('win32', 'x64'), /no server binary for win32\/x64/);
    });
});
