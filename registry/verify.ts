// Verifies a signed registry with the committed public key
// registry/signing-public-key.pem (the key the app will trust). publish.sh
// runs it before anything is read from or written to the bucket:
//
//   node --experimental-strip-types registry/verify.ts <registry.json> <registry.json.sig>
//
// Exit code 0 when the signature verifies over the file's exact bytes. A
// command line only: it runs on load, so nothing may import it.
import { existsSync, readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { verifyBytes } from './signing.ts';

const PUBLIC_KEY_PATH = join(dirname(fileURLToPath(import.meta.url)), 'signing-public-key.pem');

function main(argv: string[]): number {
  const [body, signature] = argv;
  if (!body || !signature) { console.error('usage: verify.ts <registry.json> <registry.json.sig>'); return 2; }
  if (!existsSync(PUBLIC_KEY_PATH)) { console.error('registry: registry/signing-public-key.pem is not committed yet (founder action)'); return 1; }
  if (!verifyBytes(readFileSync(body), readFileSync(signature, 'utf8'), readFileSync(PUBLIC_KEY_PATH, 'utf8'))) {
    console.error('registry: the signature does not verify over registry.json with registry/signing-public-key.pem; nothing is uploaded');
    return 1;
  }
  console.log('registry: the signature verifies with registry/signing-public-key.pem');
  return 0;
}

process.exitCode = main(process.argv.slice(2));
