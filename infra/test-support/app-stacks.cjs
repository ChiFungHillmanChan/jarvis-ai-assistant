// The stacks bin/jarvis.ts creates for one env, built from the TypeScript
// sources in memory (never from compiled .js beside them). The app is only
// constructed, with Lambda bundling skipped: nothing is synthesized or
// deployed, and no AWS lookup or credential provider runs.
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');
const cdk = require('aws-cdk-lib');

const root = path.resolve(__dirname, '..');

function load(file, cache, cdkLib) {
  if (cache.has(file)) return cache.get(file).exports;
  const source = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  }).outputText;
  const loaded = new Module(file, module);
  loaded.filename = file;
  loaded.paths = Module._nodeModulePaths(path.dirname(file));
  cache.set(file, loaded);
  loaded.require = (spec) => {
    if (spec === 'source-map-support/register') return {};
    if (spec === 'aws-cdk-lib') return cdkLib;
    const sibling = spec.startsWith('.') ? path.resolve(path.dirname(file), `${spec}.ts`) : null;
    if (sibling && fs.existsSync(sibling)) return load(sibling, cache, cdkLib);
    return Module.prototype.require.call(loaded, spec);
  };
  loaded._compile(source, file);
  return loaded.exports;
}

exports.appStackIds = function appStackIds(envName) {
  const context = { ...JSON.parse(fs.readFileSync(path.join(root, 'cdk.json'), 'utf8')).context, env: envName, 'aws:cdk:bundling-stacks': [] };
  const outdir = fs.mkdtempSync(path.join(os.tmpdir(), 'jarvis-app-stacks-test-'));
  let app;
  class FixtureApp extends cdk.App {
    constructor(props) {
      super({ ...props, context, outdir });
      app = this;
    }
  }
  const cdkLib = new Proxy(cdk, { get: (target, key) => (key === 'App' ? FixtureApp : target[key]) });
  try {
    load(path.join(root, 'bin/jarvis.ts'), new Map(), cdkLib);
    return app.node.children.filter((child) => cdk.Stack.isStack(child)).map((stack) => stack.node.id);
  } finally {
    fs.rmSync(outdir, { recursive: true, force: true });
  }
};
