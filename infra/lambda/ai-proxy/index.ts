import { CognitoJwtVerifier } from 'aws-jwt-verify';
import { clientInfo } from '../me/body';
import { converse } from './converse';
import type { Sink } from './converse';
import { createDeps } from './store';

declare const awslambda: {
  streamifyResponse(handler: (event: any, responseStream: any, context: any) => Promise<void>): unknown;
  HttpResponseStream: { from(stream: any, meta: { statusCode: number; headers: Record<string, string> }): any };
};

const verifier = CognitoJwtVerifier.create({
  userPoolId: process.env.USER_POOL_ID!,
  tokenUse: 'access',
  clientId: process.env.USER_POOL_CLIENT_ID!,
});
const deps = createDeps(process.env);

/** Never logs the token, the request body, or error text: any of them can contain user content. */
export const handler = awslambda.streamifyResponse(async (event, responseStream) => {
  let out: any = null;
  let writable = true;
  let ended = false;
  const open = (statusCode: number, headers: Record<string, string>) => {
    out = awslambda.HttpResponseStream.from(responseStream, { statusCode, headers });
    out.on?.('error', () => { writable = false; });
  };
  const end = () => {
    if (ended) return;
    ended = true;
    try { out?.end(); } catch { /* already closed */ }
  };
  const fail = (status: number, body: Record<string, unknown>) => {
    open(status, { 'content-type': 'application/json' });
    out.write(JSON.stringify(body));
    end();
  };

  try {
    if (event.requestContext?.http?.method !== 'POST' || event.rawPath !== '/v1/converse') return fail(404, { error: 'not_found' });

    const authorization: unknown = event.headers?.authorization;
    const token = typeof authorization === 'string' && authorization.startsWith('Bearer ') ? authorization.slice(7) : '';
    let userId = '';
    try {
      userId = (await verifier.verify(token)).sub;
    } catch {
      return fail(401, { error: 'unauthenticated' });
    }

    const raw: string = event.body ? (event.isBase64Encoded ? Buffer.from(event.body, 'base64').toString('utf8') : event.body) : '';
    // The same check as /me, so only a version label, never free text, reaches ai-usage.
    const { appVersion } = clientInfo(event.headers ?? {});

    const sink: Sink = {
      fail,
      start: () => open(200, { 'content-type': 'text/event-stream', 'cache-control': 'no-cache' }),
      send: e => {
        if (!writable) return;
        try {
          out.write(`data: ${JSON.stringify(e)}\n\n`, (err?: Error | null) => { if (err) writable = false; });
        } catch {
          writable = false;
        }
      },
      end,
    };
    await converse(deps, { userId, appVersion }, raw, sink);
  } catch {
    if (!out) fail(500, { error: 'upstream_error' });
  } finally {
    end();
  }
});
