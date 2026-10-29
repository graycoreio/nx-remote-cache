import { strict as assert } from 'assert';
import { describe, it } from 'node:test';

import { fetchOidcToken, readRunContext } from './github';

describe('fetchOidcToken', () => {
    it('requests a token for the Entra exchange audience', async () => {
        const requests: { url: string; auth: string }[] = [];
        const fetchImpl = (async (url: URL, init: RequestInit) => {
            requests.push({ url: url.toString(), auth: (init.headers as Record<string, string>).Authorization });
            return new Response(JSON.stringify({ value: 'jwt' }));
        }) as unknown as typeof fetch;
        const env = { ACTIONS_ID_TOKEN_REQUEST_URL: 'https://token.example/?x=1', ACTIONS_ID_TOKEN_REQUEST_TOKEN: 'rt' };
        assert.equal(await fetchOidcToken(env, fetchImpl), 'jwt');
        assert.deepEqual(requests, [{ url: 'https://token.example/?x=1&audience=api%3A%2F%2FAzureADTokenExchange', auth: 'bearer rt' }]);
    });

    it('fails on an error response', async () => {
        const fetchImpl = (async () => new Response('no', { status: 403 })) as unknown as typeof fetch;
        await assert.rejects(fetchOidcToken({ ACTIONS_ID_TOKEN_REQUEST_URL: 'https://t.example/' }, fetchImpl), /HTTP 403/);
    });
});

describe('readRunContext', () => {
    it('reads the default branch from the event payload', () => {
        const env = { GITHUB_EVENT_NAME: 'push', GITHUB_REF: 'refs/heads/main', GITHUB_EVENT_PATH: '/event.json' };
        const ctx = readRunContext(env, () => JSON.stringify({ repository: { default_branch: 'main' } }));
        assert.deepEqual(ctx, { eventName: 'push', ref: 'refs/heads/main', defaultBranch: 'main' });
        assert.equal(readRunContext(env, () => 'not json').defaultBranch, undefined);
    });
});
