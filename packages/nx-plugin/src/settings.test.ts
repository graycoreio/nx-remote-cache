import { strict as assert } from 'assert';
import { describe, it } from 'node:test';

import { resolveSettings, wantsWrite } from './settings';

const storage = { storageAccount: 'acct', storageContainer: 'nx-cache-v1' };
const github = {
    GITHUB_ACTIONS: 'true',
    ACTIONS_ID_TOKEN_REQUEST_URL: 'https://token.example/?x=1',
    ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'request-token',
    NX_REMOTE_CACHE_TENANT_ID: 'tenant',
    NX_REMOTE_CACHE_WRITER_CLIENT_ID: 'writer',
    NX_REMOTE_CACHE_READER_CLIENT_ID: 'reader',
};
const pushToMain = () => ({ eventName: 'push', ref: 'refs/heads/main', defaultBranch: 'main' });
const pullRequest = () => ({ eventName: 'pull_request', ref: 'refs/pull/7/merge', defaultBranch: 'main' });

describe('resolveSettings', () => {
    it("uses the developer's az login outside GitHub Actions", () => {
        assert.deepEqual(resolveSettings({ ...storage, storagePrefix: 'v2' }, {}, pullRequest), {
            settings: { account: 'acct', container: 'nx-cache-v1', prefix: 'v2', mode: 'developer_tools' },
        });
    });

    it('stays off until storage is configured', () => {
        assert.match((resolveSettings(undefined, {}, pullRequest) as { disabled: string }).disabled, /no storage configured/);
        // GitHub expands a missing repository variable to an empty string.
        const empty = { NX_REMOTE_CACHE_STORAGE_ACCOUNT: '', NX_REMOTE_CACHE_STORAGE_CONTAINER: '' };
        assert.ok('disabled' in resolveSettings(undefined, empty, pullRequest));
    });

    it('takes every option from the environment, which overrides nx.json', () => {
        const env = {
            NX_REMOTE_CACHE_STORAGE_ACCOUNT: 'env-acct',
            NX_REMOTE_CACHE_STORAGE_CONTAINER: 'env-container',
            NX_REMOTE_CACHE_STORAGE_PREFIX: 'env-prefix',
        };
        const expected = { settings: { account: 'env-acct', container: 'env-container', prefix: 'env-prefix', mode: 'developer_tools' } };
        assert.deepEqual(resolveSettings(undefined, env, pullRequest), expected);
        assert.deepEqual(resolveSettings({ ...storage, storagePrefix: 'v2' }, env, pullRequest), expected);
        const read = resolveSettings({ ...storage, access: 'write' }, { ...github, NX_REMOTE_CACHE_ACCESS: 'read' }, pushToMain);
        assert.equal('settings' in read && read.settings.mode === 'github' && read.settings.clientId, 'reader');
    });

    it('can be switched off, and defers to a server the user configured', () => {
        assert.ok('disabled' in resolveSettings(storage, { NX_REMOTE_CACHE: 'off' }, pullRequest));
        assert.ok('disabled' in resolveSettings(storage, { NX_SELF_HOSTED_REMOTE_CACHE_SERVER: 'http://x' }, pullRequest));
    });

    it('asks for the reader identity on a pull request', () => {
        assert.deepEqual(resolveSettings(storage, github, pullRequest), {
            settings: { account: 'acct', container: 'nx-cache-v1', prefix: undefined, mode: 'github', tenantId: 'tenant', clientId: 'reader', write: false },
        });
    });

    it('asks for the writer identity on a push to the default branch, unless told to read', () => {
        const write = resolveSettings(storage, github, pushToMain);
        assert.equal('settings' in write && write.settings.mode === 'github' && write.settings.clientId, 'writer');
        const read = resolveSettings(storage, { ...github, NX_REMOTE_CACHE_ACCESS: 'read' }, pushToMain);
        assert.equal('settings' in read && read.settings.mode === 'github' && read.settings.clientId, 'reader');
    });

    it('stays off in a job without an OIDC token, such as a fork PR', () => {
        const resolved = resolveSettings(storage, { GITHUB_ACTIONS: 'true' }, pullRequest);
        assert.match((resolved as { disabled: string }).disabled, /OIDC token/);
    });

    it('needs the identity it would ask for', () => {
        const { NX_REMOTE_CACHE_WRITER_CLIENT_ID, ...noWriter } = github;
        const resolved = resolveSettings(storage, noWriter, pushToMain);
        assert.match((resolved as { disabled: string }).disabled, /NX_REMOTE_CACHE_WRITER_CLIENT_ID/);
        assert.ok('settings' in resolveSettings(storage, noWriter, pullRequest));
    });

    it('rejects an unknown access', () => {
        assert.throws(() => resolveSettings(storage, { ...github, NX_REMOTE_CACHE_ACCESS: 'admin' }, pullRequest), /access must be/);
    });
});

describe('wantsWrite', () => {
    it('selects the writer only for a push to the default branch', () => {
        assert.equal(wantsWrite('auto', pushToMain()), true);
        assert.equal(wantsWrite('auto', { eventName: 'push', ref: 'refs/heads/feature', defaultBranch: 'main' }), false);
        assert.equal(wantsWrite('auto', { eventName: 'push', ref: 'refs/heads/main' }), false);
        assert.equal(wantsWrite('auto', pullRequest()), false);
        assert.equal(wantsWrite('write', pullRequest()), true);
        assert.equal(wantsWrite('read', pushToMain()), false);
    });
});
