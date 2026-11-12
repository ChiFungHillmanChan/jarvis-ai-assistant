#!/usr/bin/env node
import 'source-map-support/register';
import * as cdk from 'aws-cdk-lib';
import { AuthStack } from '../lib/auth-stack';
import { DataStack } from '../lib/data-stack';
import { ObservabilityStack } from '../lib/observability-stack';
import { PaymentStack } from '../lib/payment-stack';
import { AuditStack } from '../lib/audit-stack';
import { AiProxyStack } from '../lib/ai-proxy-stack';

const app = new cdk.App();

const env = (app.node.tryGetContext('env') as string | undefined) ?? 'sandbox';
const validEnvs = ['sandbox', 'dev', 'prod'];
if (!validEnvs.includes(env)) {
  throw new Error(`Invalid env "${env}". Must be one of: ${validEnvs.join(', ')}`);
}

const ctx = app.node.tryGetContext('jarvis');
const account = process.env.CDK_DEFAULT_ACCOUNT ?? '759626151650';
const region = ctx.primaryRegion as string;

const stackProps: cdk.StackProps = {
  env: { account, region },
  tags: {
    Project: 'jarvis',
    Env: env,
    ManagedBy: 'cdk',
    Owner: 'hillmanchan709@gmail.com',
  },
};

const auth = new AuthStack(app, `Jarvis-${env}-Auth`, {
  ...stackProps,
  envName: env,
  appDomain: ctx.appDomain,
});

const data = new DataStack(app, `Jarvis-${env}-Data`, {
  ...stackProps,
  envName: env,
  kmsKey: auth.dataKey,
});

new ObservabilityStack(app, `Jarvis-${env}-Observability`, {
  ...stackProps,
  envName: env,
  billingAlertEmail: ctx.billingAlertEmail,
  syncBucketName: data.syncBucketName,
});

new PaymentStack(app, `Jarvis-${env}-Payment`, {
  ...stackProps,
  envName: env,
  userMetaTable: data.userMetaTable,
  userPool: auth.userPool,
  userPoolClient: auth.userPoolClient,
  aiTables: data.aiTables,
});

new AuditStack(app, `Jarvis-${env}-Audit`, {
  ...stackProps,
  envName: env,
  kmsKey: auth.dataKey,
});

// Sandbox only: the Bedrock budget and kill switch exist only there, so dev/prod get a proxy in a later PR together with their own.
if (env === 'sandbox') {
  new AiProxyStack(app, `Jarvis-${env}-AiProxy`, { ...stackProps, envName: env, kmsKey: auth.dataKey, userPool: auth.userPool, userPoolClient: auth.userPoolClient, userMetaTable: data.userMetaTable, aiTables: data.aiTables, billingAlertEmail: ctx.billingAlertEmail });
}
