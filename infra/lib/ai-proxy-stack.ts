import * as cdk from 'aws-cdk-lib';
import * as budgets from 'aws-cdk-lib/aws-budgets';
import * as cw from 'aws-cdk-lib/aws-cloudwatch';
import * as cwActions from 'aws-cdk-lib/aws-cloudwatch-actions';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as ddb from 'aws-cdk-lib/aws-dynamodb';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as kms from 'aws-cdk-lib/aws-kms';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as nodejs from 'aws-cdk-lib/aws-lambda-nodejs';
import * as logs from 'aws-cdk-lib/aws-logs';
import * as sns from 'aws-cdk-lib/aws-sns';
import * as snsSubs from 'aws-cdk-lib/aws-sns-subscriptions';
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

    this.addGuardrails(props, logGroup);

    new cdk.CfnOutput(this, 'AiProxyUrl', {
      value: this.proxyUrl,
      description: 'POST streaming AI requests here with a Cognito access token',
      exportName: `Jarvis-${props.envName}-AiProxyUrl`,
    });
  }

  private addGuardrails(props: AiProxyStackProps, logGroup: logs.ILogGroup): void {
    const alertTopic = new sns.Topic(this, 'AiAlertTopic', {
      topicName: `jarvis-${props.envName}-ai-alerts`,
      displayName: `JARVIS ${props.envName} AI alerts`,
    });
    alertTopic.addSubscription(new snsSubs.EmailSubscription(props.billingAlertEmail));

    // The proxy writes this EMF metric once a user's billed amount passes US$20 in a Hong Kong day.
    new cw.Alarm(this, 'UserDailyOver20Alarm', {
      alarmName: `jarvis-${props.envName}-ai-user-daily-over-20usd`,
      metric: new cw.Metric({
        namespace: 'Jarvis/AiProxy',
        metricName: 'UserDailyBilledOver20',
        dimensionsMap: { Env: props.envName },
        statistic: 'Sum',
        period: cdk.Duration.minutes(5),
      }),
      threshold: 1,
      evaluationPeriods: 1,
      comparisonOperator: cw.ComparisonOperator.GREATER_THAN_OR_EQUAL_TO_THRESHOLD,
      treatMissingData: cw.TreatMissingData.NOT_BREACHING,
    }).addAlarmAction(new cwActions.SnsAction(alertTopic));

    // The proxy writes this EMF metric once the billed total across all users reaches globalDailyCapMicros in a Hong Kong day.
    // From then on it pauses every request until the next Hong Kong day.
    new cw.Alarm(this, 'GlobalDailyCeilingAlarm', {
      alarmName: `jarvis-${props.envName}-ai-global-daily-ceiling`,
      metric: new cw.Metric({
        namespace: 'Jarvis/AiProxy',
        metricName: 'GlobalDailyCeilingReached',
        dimensionsMap: { Env: props.envName },
        statistic: 'Sum',
        period: cdk.Duration.minutes(5),
      }),
      threshold: 1,
      evaluationPeriods: 1,
      comparisonOperator: cw.ComparisonOperator.GREATER_THAN_OR_EQUAL_TO_THRESHOLD,
      treatMissingData: cw.TreatMissingData.NOT_BREACHING,
    }).addAlarmAction(new cwActions.SnsAction(alertTopic));

    // A served request whose meter write failed twice is uncounted, so it no longer counts towards the cap.
    // Metric filter dimensions can only come from log fields and the converse log has no env, so the env is in the metric name.
    const meterFailed = new logs.MetricFilter(this, 'MeterFailedFilter', {
      logGroup,
      filterPattern: logs.FilterPattern.all(
        logs.FilterPattern.stringValue('$.event', '=', 'converse'),
        logs.FilterPattern.booleanValue('$.meterFailed', true),
      ),
      metricNamespace: 'Jarvis/AiProxy',
      metricName: `MeterFailed-${props.envName}`,
      metricValue: '1',
      unit: cw.Unit.COUNT,
    });
    new cw.Alarm(this, 'MeterFailedAlarm', {
      alarmName: `jarvis-${props.envName}-ai-meter-failed`,
      metric: meterFailed.metric({ statistic: 'Sum', period: cdk.Duration.minutes(5) }),
      threshold: 1,
      evaluationPeriods: 1,
      comparisonOperator: cw.ComparisonOperator.GREATER_THAN_OR_EQUAL_TO_THRESHOLD,
      treatMissingData: cw.TreatMissingData.NOT_BREACHING,
    }).addAlarmAction(new cwActions.SnsAction(alertTopic));

    // Account-wide Bedrock budget: one per account, so only the sandbox stack owns it.
    if (props.envName !== 'sandbox') return;

    const killSwitch = new iam.ManagedPolicy(this, 'BedrockKillSwitch', {
      managedPolicyName: 'jarvis-bedrock-kill-switch',
      statements: [new iam.PolicyStatement({
        effect: iam.Effect.DENY,
        actions: ['bedrock:InvokeModel*'],
        resources: ['*'],
      })],
    });

    const budgetRole = new iam.Role(this, 'BedrockBudgetActionRole', {
      assumedBy: new iam.ServicePrincipal('budgets.amazonaws.com'),
    });
    budgetRole.addToPolicy(new iam.PolicyStatement({
      actions: ['iam:AttachRolePolicy', 'iam:DetachRolePolicy'],
      resources: [this.proxyRole.roleArn],
      conditions: { ArnEquals: { 'iam:PolicyARN': killSwitch.managedPolicyArn } },
    }));

    const subscribers = [{ subscriptionType: 'EMAIL', address: props.billingAlertEmail }];
    const budget = new budgets.CfnBudget(this, 'BedrockMonthlyBudget', {
      budget: {
        budgetName: 'jarvis-bedrock-monthly',
        budgetType: 'COST',
        timeUnit: 'MONTHLY',
        budgetLimit: { amount: 100, unit: 'USD' },
        costFilters: { Service: ['Amazon Bedrock'] },
        // Count real spend only: with credits included, the alerts and the kill switch would fire late.
        costTypes: { includeCredit: false, includeRefund: false },
      },
      notificationsWithSubscribers: [50, 80, 100].map(threshold => ({
        notification: {
          notificationType: 'ACTUAL',
          comparisonOperator: 'GREATER_THAN',
          threshold,
          thresholdType: 'PERCENTAGE',
        },
        subscribers,
      })),
    });

    const action = new budgets.CfnBudgetsAction(this, 'BedrockKillSwitchAction', {
      budgetName: 'jarvis-bedrock-monthly',
      notificationType: 'ACTUAL',
      actionType: 'APPLY_IAM_POLICY',
      actionThreshold: { type: 'PERCENTAGE', value: 150 },
      approvalModel: 'AUTOMATIC',
      executionRoleArn: budgetRole.roleArn,
      definition: {
        iamActionDefinition: { policyArn: killSwitch.managedPolicyArn, roles: [this.proxyRole.roleName] },
      },
      subscribers: [{ type: 'EMAIL', address: props.billingAlertEmail }],
    });
    action.addDependency(budget);
    // GetAtt on the role does not wait for its attach/detach policy; Budgets may check it on create.
    action.node.addDependency(budgetRole);
  }
}
