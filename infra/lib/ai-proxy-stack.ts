import * as cdk from 'aws-cdk-lib';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as ddb from 'aws-cdk-lib/aws-dynamodb';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as kms from 'aws-cdk-lib/aws-kms';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as nodejs from 'aws-cdk-lib/aws-lambda-nodejs';
import * as logs from 'aws-cdk-lib/aws-logs';
import { Construct } from 'constructs';
import * as path from 'path';
import type { AiTables } from './data-stack';
import aiConfig from '../config/jarvis-ai.json';

export interface AiProxyStackProps extends cdk.StackProps {
  envName: string;
  kmsKey: kms.IKey;
  userPool: cognito.IUserPool;
  userPoolClient: cognito.IUserPoolClient;
  userMetaTable: ddb.ITable;
  aiTables: AiTables;
  // Used by the cost guardrails added to this stack in a later task.
  billingAlertEmail: string;
}

export class AiProxyStack extends cdk.Stack {
  public readonly proxyRole: iam.Role;
  public readonly proxyUrl: string;

  constructor(scope: Construct, id: string, props: AiProxyStackProps) {
    super(scope, id, props);

    const isProd = props.envName === 'prod';

    const limitsTable = new ddb.Table(this, 'AiLimitsTable', {
      tableName: `jarvis-${props.envName}-ai-limits`,
      partitionKey: { name: 'pk', type: ddb.AttributeType.STRING },
      billingMode: ddb.BillingMode.PAY_PER_REQUEST,
      encryption: ddb.TableEncryption.CUSTOMER_MANAGED,
      encryptionKey: props.kmsKey,
      pointInTimeRecoverySpecification: { pointInTimeRecoveryEnabled: isProd },
      timeToLiveAttribute: 'expiresAt',
      removalPolicy: isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY,
    });

    this.proxyRole = new iam.Role(this, 'AiProxyRole', {
      roleName: `jarvis-${props.envName}-ai-proxy`,
      assumedBy: new iam.ServicePrincipal('lambda.amazonaws.com'),
      managedPolicies: [iam.ManagedPolicy.fromAwsManagedPolicyName('service-role/AWSLambdaBasicExecutionRole')],
    });

    const logGroup = new logs.LogGroup(this, 'AiProxyLogs', {
      logGroupName: `/aws/lambda/jarvis-${props.envName}-ai-proxy`,
      retention: logs.RetentionDays.ONE_MONTH,
      removalPolicy: cdk.RemovalPolicy.DESTROY,
    });

    const proxyFn = new nodejs.NodejsFunction(this, 'AiProxyFn', {
      functionName: `jarvis-${props.envName}-ai-proxy`,
      runtime: lambda.Runtime.NODEJS_24_X,
      entry: path.join(__dirname, '../lambda/ai-proxy/index.ts'),
      handler: 'handler',
      role: this.proxyRole,
      timeout: cdk.Duration.seconds(300),
      memorySize: 512,
      reservedConcurrentExecutions: 50,
      logGroup,
      environment: {
        USER_POOL_ID: props.userPool.userPoolId,
        USER_POOL_CLIENT_ID: props.userPoolClient.userPoolClientId,
        USER_META_TABLE: props.userMetaTable.tableName,
        AI_USAGE_TABLE: props.aiTables.usage.tableName,
        AI_PERIOD_TABLE: props.aiTables.period.tableName,
        AI_GRANTS_TABLE: props.aiTables.grants.tableName,
        AI_CONFIG_TABLE: props.aiTables.config.tableName,
        AI_LIMITS_TABLE: limitsTable.tableName,
        ENV: props.envName,
      },
      bundling: { minify: true, sourceMap: true },
    });

    props.userMetaTable.grantReadData(proxyFn);
    props.aiTables.grants.grantReadData(proxyFn);
    props.aiTables.config.grantReadData(proxyFn);
    props.aiTables.period.grantReadWriteData(proxyFn);
    limitsTable.grantReadWriteData(proxyFn);
    props.aiTables.usage.grantWriteData(proxyFn);

    // Bedrock is called only in the regions and models of the catalog, never '*'.
    this.proxyRole.addToPolicy(new iam.PolicyStatement({
      actions: ['bedrock:InvokeModel', 'bedrock:InvokeModelWithResponseStream'],
      resources: Object.entries(aiConfig.catalog).map(
        ([modelId, model]) => `arn:aws:bedrock:${model.region}::foundation-model/${modelId}`,
      ),
    }));

    // The handler verifies the Cognito access token itself, so the URL is not IAM-gated.
    const url = proxyFn.addFunctionUrl({
      authType: lambda.FunctionUrlAuthType.NONE,
      invokeMode: lambda.InvokeMode.RESPONSE_STREAM,
    });
    this.proxyUrl = url.url;

    new cdk.CfnOutput(this, 'AiProxyUrl', {
      value: this.proxyUrl,
      description: 'POST streaming AI requests here with a Cognito access token',
      exportName: `Jarvis-${props.envName}-AiProxyUrl`,
    });
  }
}
