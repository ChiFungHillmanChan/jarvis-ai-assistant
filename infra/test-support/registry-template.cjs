// Synthesize the real Registry stack with a fixture account. No AWS lookup,
// credential provider, change set or deployment runs in this helper, and the
// Lambda is not bundled (its code is tested on its own).
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');
const cdk = require('aws-cdk-lib');
const { Template } = require('aws-cdk-lib/assertions');

exports.registryTemplate = function registryTemplate(envName = 'dev') {
  const file = path.resolve(__dirname, '../lib/registry-stack.ts');
  const source = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  }).outputText;
  const loaded = new Module(file, module);
  loaded.filename = file;
  loaded.paths = Module._nodeModulePaths(path.dirname(file));
  loaded._compile(source, file);
  const outdir = fs.mkdtempSync('/tmp/jarvis-registry-template-test-');
  try {
    const app = new cdk.App({ outdir, context: { 'aws:cdk:bundling-stacks': [] } });
    const env = { account: '111111111111', region: 'ap-southeast-1' };
    const stack = new loaded.exports.RegistryStack(app, 'FixtureRegistry', { env, envName, githubRepo: 'owner/repo' });
    return Template.fromStack(stack).toJSON();
  } finally {
    fs.rmSync(outdir, { recursive: true, force: true });
  }
};
