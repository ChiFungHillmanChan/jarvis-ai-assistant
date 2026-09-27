// Synthesize the real Payment stack against local fixture resources. No AWS
// lookup, credential provider, change set or deployment runs in this helper.
const fs = require('node:fs');
const path = require('node:path');
const Module = require('node:module');
const ts = require('typescript');
const cdk = require('aws-cdk-lib');
const { Template } = require('aws-cdk-lib/assertions');

exports.paymentTemplate = function paymentTemplate(extraContext = {}) {
  const file = path.resolve(__dirname, '../lib/payment-stack.ts');
  const source = ts.transpileModule(fs.readFileSync(file, 'utf8'), {
    compilerOptions: { module: ts.ModuleKind.CommonJS, target: ts.ScriptTarget.ES2022, esModuleInterop: true },
  }).outputText;
  const loaded = new Module(file, module);
  loaded.filename = file;
  loaded.paths = Module._nodeModulePaths(path.dirname(file));
  loaded._compile(source, file);
  const app = new cdk.App({
    outdir: fs.mkdtempSync('/tmp/jarvis-payment-template-test-'),
    context: { stripeProPriceId: 'price_fixture_pro', stripeTeamPriceId: 'price_fixture_team', stripePortalConfigurationId: 'bpc_fixture', ...extraContext },
  });
  const env = { account: '111111111111', region: 'ap-southeast-1' };
  const base = new cdk.Stack(app, 'FixtureBase', { env });
  const stack = new loaded.exports.PaymentStack(app, 'FixturePayment', {
    env, envName: 'sandbox',
    userMetaTable: cdk.aws_dynamodb.Table.fromTableName(base, 'Users', 'fixture-users'),
    userPool: cdk.aws_cognito.UserPool.fromUserPoolId(base, 'Pool', 'ap-southeast-1_fixture'),
    userPoolClient: cdk.aws_cognito.UserPoolClient.fromUserPoolClientId(base, 'Client', 'fixture-client'),
  });
  return Template.fromStack(stack).toJSON();
};
