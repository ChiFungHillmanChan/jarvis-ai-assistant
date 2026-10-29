#!/usr/bin/env node
// Sandbox access-token helper for proxy smoke tests.
//
// Runs a Cognito PKCE authorization-code flow against the sandbox user pool,
// then writes ONLY the access token to the file given as argv[2] (mode 0600)
// and prints the token's `sub`. Quit the JARVIS app first so port 47821 is free.
//
// Usage: node scripts/sandbox-access-token.mjs <output-file>

import { createServer } from 'node:http';
import { createHash, randomBytes } from 'node:crypto';
import { chmodSync, readFileSync, writeFileSync } from 'node:fs';

const PORT = 47821;
const HOST = '127.0.0.1';
const REDIRECT_URI = `http://${HOST}:${PORT}/callback`;
const SCOPE = 'openid email profile';
const TIMEOUT_MS = 5 * 60 * 1000;

const outPath = process.argv[2];
if (!outPath) {
  console.error('Usage: node scripts/sandbox-access-token.mjs <output-file>');
  process.exit(1);
}

function fail(message) {
  console.error(message);
  process.exit(1);
}

const outputs = JSON.parse(readFileSync(new URL('../cdk-outputs.json', import.meta.url), 'utf8'));
const auth = outputs['Jarvis-sandbox-Auth'];
if (!auth?.UserPoolDomainUrl || !auth?.UserPoolClientId) {
  fail('cdk-outputs.json is missing Jarvis-sandbox-Auth UserPoolDomainUrl or UserPoolClientId');
}
const domain = String(auth.UserPoolDomainUrl).replace(/\/+$/, '');
const clientId = auth.UserPoolClientId;

const base64url = (buf) => buf.toString('base64url');
const codeVerifier = base64url(randomBytes(48)); // 64 chars
const codeChallenge = base64url(createHash('sha256').update(codeVerifier).digest());
const state = base64url(randomBytes(16));

const loginUrl =
  `${domain}/oauth2/authorize?` +
  new URLSearchParams({
    response_type: 'code',
    client_id: clientId,
    redirect_uri: REDIRECT_URI,
    scope: SCOPE,
    state,
    code_challenge: codeChallenge,
    code_challenge_method: 'S256',
  }).toString();

function waitForCode() {
  return new Promise((resolve, reject) => {
    const server = createServer((req, res) => {
      const url = new URL(req.url ?? '/', `http://${HOST}:${PORT}`);
      if (url.pathname !== '/callback') {
        res.writeHead(404, { 'Content-Type': 'text/plain' });
        res.end('Not found');
        return;
      }
      const error = url.searchParams.get('error');
      const code = url.searchParams.get('code');
      const gotState = url.searchParams.get('state');
      let outcome;
      if (error) {
        outcome = { err: `Sign-in returned error: ${error}` };
      } else if (gotState !== state) {
        outcome = { err: 'State mismatch on callback; rejecting.' };
      } else if (!code) {
        outcome = { err: 'Callback had no code.' };
      } else {
        outcome = { code };
      }
      res.writeHead(outcome.err ? 400 : 200, { 'Content-Type': 'text/plain' });
      res.end(outcome.err ? 'Sign-in failed. Return to the terminal.' : 'Signed in. You can close this tab.');
      clearTimeout(timer);
      server.close();
      server.closeAllConnections();
      if (outcome.err) reject(new Error(outcome.err));
      else resolve(outcome.code);
    });

    const timer = setTimeout(() => {
      server.close();
      server.closeAllConnections();
      reject(new Error('Timed out waiting for sign-in (5 minutes).'));
    }, TIMEOUT_MS);

    server.on('error', (err) => {
      clearTimeout(timer);
      reject(new Error(`Could not listen on ${HOST}:${PORT}: ${err.code ?? err.message}`));
    });

    server.listen(PORT, HOST, () => {
      console.log('Open this URL in your browser and sign in to the sandbox account:\n');
      console.log(loginUrl);
      console.log(`\nWaiting for the callback on ${REDIRECT_URI} ...`);
    });
  });
}

async function exchange(code) {
  const res = await fetch(`${domain}/oauth2/token`, {
    method: 'POST',
    headers: { 'Content-Type': 'application/x-www-form-urlencoded' },
    body: new URLSearchParams({
      grant_type: 'authorization_code',
      client_id: clientId,
      code,
      redirect_uri: REDIRECT_URI,
      code_verifier: codeVerifier,
    }).toString(),
  });
  let body = null;
  try {
    body = await res.json();
  } catch {
    // non-JSON body; never print it
  }
  if (!res.ok) {
    const oauthError = typeof body?.error === 'string' ? body.error : 'unknown';
    fail(`Token exchange failed: HTTP ${res.status}, error=${oauthError}`);
  }
  if (typeof body?.access_token !== 'string') {
    fail('Token exchange succeeded but the response had no access_token.');
  }
  return body.access_token;
}

function subOf(jwt) {
  try {
    const payload = JSON.parse(Buffer.from(jwt.split('.')[1], 'base64url').toString('utf8'));
    return typeof payload.sub === 'string' ? payload.sub : '(no sub claim)';
  } catch {
    return '(could not decode sub)';
  }
}

try {
  const code = await waitForCode();
  const accessToken = await exchange(code);
  writeFileSync(outPath, accessToken, { mode: 0o600 });
  chmodSync(outPath, 0o600);
  console.log(`Access token written to ${outPath} (mode 0600).`);
  console.log(`sub: ${subOf(accessToken)}`);
  process.exit(0);
} catch (err) {
  fail(err instanceof Error ? err.message : 'Unexpected error.');
}
