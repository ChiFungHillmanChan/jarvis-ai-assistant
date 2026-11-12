// Synthesize one real stack from lib/ against local fixture resources. No AWS
// lookup, credential provider, change set or deployment runs in this helper.
// makeProps(base, cdk) is the only source of the stack's props, so a test can
// build any environment it needs; `env` is added here.
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');
const cdk = require('aws-cdk-lib');
const { Template } = require('aws-cdk-lib/assertions');

exports.synthStack = function synthStack(stackFile, className, makeProps, context = {}) {
  const file = path.resolve(__dirname, '..', stackFile);
  const source = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  }).outputText;
  const loaded = new Module(file, module);
  loaded.filename = file;
  loaded.paths = Module._nodeModulePaths(path.dirname(file));
  loaded._compile(source, file);
  const app = new cdk.App({ outdir: fs.mkdtempSync('/tmp/jarvis-stack-template-test-'), context });
  const env = { account: '111111111111', region: 'ap-southeast-1' };
  const base = new cdk.Stack(app, 'FixtureBase', { env });
  const stack = new loaded.exports[className](app, `Fixture${className}`, { env, ...makeProps(base, cdk) });
  return Template.fromStack(stack).toJSON();
};
