// registry/publish.sh against a stand-in `aws` command that keeps objects in a
// temporary directory. Nothing here reaches AWS. The signature is made with a
// throwaway key held in memory; only the signed bytes and the signature are
// written to the temporary directory.
import test from 'node:test';
import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, writeFileSync, chmodSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { generateKeyPairSync } from 'node:crypto';
import { canonicalBytes, signBytes } from '../registry/signing.ts';

const script = new URL('../registry/publish.sh', import.meta.url).pathname;
const seeded = () => JSON.parse(readFileSync(new URL('../registry/registry.json', import.meta.url), 'utf8'));
const privateKey = generateKeyPairSync('ed25519').privateKey.export({ type: 'pkcs8', format: 'pem' }) as string;

// list-objects-v2 answers the publish script's one query (is this exact key
// there?); put-object honours --if-none-match '*' as S3 does (412).
const FAKE_AWS = `#!/usr/bin/env node
const fs = require('fs'), path = require('path');
const [, , service, command, ...rest] = process.argv;
const flag = name => { const i = rest.indexOf(name); return i < 0 ? undefined : rest[i + 1]; };
const root = process.env.FAKE_S3;
const file = key => path.join(root, key);
fs.appendFileSync(path.join(root, '..', 'calls.log'), [command, flag('--key') ?? flag('--prefix')].join(' ') + '\\n');
if (service !== 's3api' || flag('--bucket') !== 'fixture-bucket') { console.error('unexpected call'); process.exit(2); }
if (command === 'list-objects-v2') { console.log(fs.existsSync(file(flag('--prefix'))) ? '1' : '0'); process.exit(0); }
if (command === 'get-object') {
  const key = flag('--key');
  if (!fs.existsSync(file(key))) { console.error('NoSuchKey'); process.exit(254); }
  fs.copyFileSync(file(key), rest[rest.length - 1]); console.log('{}'); process.exit(0);
}
if (command === 'put-object') {
  const key = flag('--key');
  if (flag('--if-none-match') === '*' && fs.existsSync(file(key))) { console.error('PreconditionFailed (412)'); process.exit(254); }
  fs.mkdirSync(path.dirname(file(key)), { recursive: true });
  fs.copyFileSync(flag('--body'), file(key));
  fs.writeFileSync(file(key) + '.meta', JSON.stringify({ type: flag('--content-type'), metadata: JSON.parse(flag('--metadata') ?? '{}') }));
  console.log('{}'); process.exit(0);
}
console.error('unexpected command'); process.exit(2);
`;

function setup() {
  const root = mkdtempSync(join(tmpdir(), 'jarvis-registry-publish-'));
  mkdirSync(join(root, 'bin'));
  mkdirSync(join(root, 's3'));
  writeFileSync(join(root, 'bin', 'aws'), FAKE_AWS);
  chmodSync(join(root, 'bin', 'aws'), 0o755);
  writeFileSync(join(root, 'calls.log'), '');
  const sign = (doc: unknown, name: string) => {
    const dir = join(root, name);
    mkdirSync(dir);
    const bytes = canonicalBytes(doc);
    writeFileSync(join(dir, 'registry.json'), bytes);
    writeFileSync(join(dir, 'registry.json.sig'), signBytes(bytes, privateKey));
    return dir;
  };
  const publish = (dir: string) => spawnSync('bash', [script, dir], {
    encoding: 'utf8',
    env: { ...process.env, PATH: `${join(root, 'bin')}:${process.env.PATH}`, FAKE_S3: join(root, 's3'), REGISTRY_BUCKET: 'fixture-bucket' },
  });
  const object = (key: string) => readFileSync(join(root, 's3', key));
  const meta = (key: string) => JSON.parse(readFileSync(join(root, 's3', `${key}.meta`), 'utf8'));
  const puts = () => readFileSync(join(root, 'calls.log'), 'utf8').split('\n').filter(line => line.startsWith('put-object'));
  return { root, sign, publish, object, meta, puts, done: () => rmSync(root, { recursive: true, force: true }) };
}

test('publishing writes the version objects once, then latest with the signature as metadata', (t) => {
  const s = setup();
  t.after(s.done);
  const v1 = s.sign(seeded(), 'v1');
  const first = s.publish(v1);
  assert.equal(first.status, 0, first.stderr);
  assert.match(first.stdout, /nothing published yet/);
  const bytes = readFileSync(join(v1, 'registry.json'));
  const signature = readFileSync(join(v1, 'registry.json.sig'), 'utf8');
  assert.deepEqual(s.object('registry/v1.json'), bytes);
  assert.equal(s.object('registry/v1.json.sig').toString(), signature);
  assert.deepEqual(s.object('registry/latest.json'), bytes);
  assert.equal(s.object('registry/latest.json.sig').toString(), signature);
  assert.deepEqual(s.meta('registry/latest.json'), { type: 'application/json', metadata: { signature, version: '1' } });
  assert.deepEqual(s.puts().map(line => line.split(' ')[1]), ['registry/v1.json', 'registry/v1.json.sig', 'registry/latest.json.sig', 'registry/latest.json']);

  // Running the same publish again changes nothing.
  const again = s.publish(v1);
  assert.equal(again.status, 0, again.stderr);
  assert.match(again.stdout, /already the published version/);
  assert.equal(s.puts().length, 4);

  // A higher version moves latest and keeps v1.
  const doc = seeded();
  doc.version = 2;
  const v2 = s.sign(doc, 'v2');
  const second = s.publish(v2);
  assert.equal(second.status, 0, second.stderr);
  assert.deepEqual(s.object('registry/latest.json'), readFileSync(join(v2, 'registry.json')));
  assert.deepEqual(s.object('registry/v1.json'), bytes);
  assert.equal(s.meta('registry/latest.json').metadata.version, '2');
});

test('a version that is not above the published one is refused and latest is untouched', (t) => {
  const s = setup();
  t.after(s.done);
  const doc = seeded();
  doc.version = 5;
  assert.equal(s.publish(s.sign(doc, 'v5')).status, 0);
  const published = s.object('registry/latest.json');
  const putsBefore = s.puts().length;

  const same = seeded();
  same.version = 5;
  same.managed.paused = true;
  const refused = s.publish(s.sign(same, 'v5-changed'));
  assert.notEqual(refused.status, 0);
  assert.match(refused.stderr, /must be greater than the published version 5/);

  const lower = seeded();
  lower.version = 4;
  assert.notEqual(s.publish(s.sign(lower, 'v4')).status, 0);
  assert.deepEqual(s.object('registry/latest.json'), published);
  assert.equal(s.puts().length, putsBefore);
});

test('an existing version object with other content is never replaced', (t) => {
  const s = setup();
  t.after(s.done);
  // v3.json exists (say, from an interrupted run of different content) while latest is v2.
  const v2 = seeded();
  v2.version = 2;
  assert.equal(s.publish(s.sign(v2, 'v2')).status, 0);
  mkdirSync(join(s.root, 's3', 'registry'), { recursive: true });
  writeFileSync(join(s.root, 's3', 'registry', 'v3.json'), '{"version":3}');
  const v3 = seeded();
  v3.version = 3;
  const result = s.publish(s.sign(v3, 'v3'));
  assert.notEqual(result.status, 0);
  assert.match(result.stderr, /already exists with other content/);
  assert.equal(s.meta('registry/latest.json').metadata.version, '2');
  assert.ok(!existsSync(join(s.root, 's3', 'registry', 'v3.json.sig')));
});

test('publish refuses a missing bucket name or a malformed signature', (t) => {
  const s = setup();
  t.after(s.done);
  const dir = s.sign(seeded(), 'v1');
  const noBucket = spawnSync('bash', [script, dir], { encoding: 'utf8', env: { ...process.env, PATH: `${join(s.root, 'bin')}:${process.env.PATH}`, FAKE_S3: join(s.root, 's3'), REGISTRY_BUCKET: '' } });
  assert.match(noBucket.stderr, /REGISTRY_BUCKET is not set/);
  assert.notEqual(noBucket.status, 0);
  writeFileSync(join(dir, 'registry.json.sig'), 'not a signature');
  const bad = s.publish(dir);
  assert.notEqual(bad.status, 0);
  assert.match(bad.stderr, /bad signature file/);
  assert.equal(s.puts().length, 0);
});
