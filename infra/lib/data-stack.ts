import * as cdk from 'aws-cdk-lib';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as kms from 'aws-cdk-lib/aws-kms';
import * as ddb from 'aws-cdk-lib/aws-dynamodb';
import { Construct } from 'constructs';

export interface DataStackProps extends cdk.StackProps {
  envName: string;
  kmsKey: kms.IKey;
}

export interface AiTables {
  usage: ddb.ITable;
  period: ddb.ITable;
  grants: ddb.ITable;
  config: ddb.ITable;
}

export class DataStack extends cdk.Stack {
  public readonly syncBucket: s3.Bucket;
  public readonly syncBucketName: string;
  public readonly userMetaTable: ddb.Table;
  public readonly pluginRegistryTable: ddb.Table;
  public readonly aiTables: AiTables;

  constructor(scope: Construct, id: string, props: DataStackProps) {
    super(scope, id, props);

    const isProd = props.envName === 'prod';
    const removal = isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY;

    this.syncBucket = new s3.Bucket(this, 'JarvisSyncBucket', {
      bucketName: `jarvis-sync-${props.envName}-${this.account}`,
      encryption: s3.BucketEncryption.KMS,
      encryptionKey: props.kmsKey,
      bucketKeyEnabled: true,
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      enforceSSL: true,
      versioned: true,
      lifecycleRules: [
        {
          id: 'transition-to-glacier',
          enabled: true,
          transitions: [
            {
              storageClass: s3.StorageClass.GLACIER_INSTANT_RETRIEVAL,
              transitionAfter: cdk.Duration.days(90),
            },
          ],
          noncurrentVersionExpiration: cdk.Duration.days(30),
        },
      ],
      removalPolicy: removal,
      autoDeleteObjects: !isProd,
    });
    this.syncBucketName = this.syncBucket.bucketName;

    this.userMetaTable = new ddb.Table(this, 'UserMetaTable', {
      tableName: `jarvis-${props.envName}-user-meta`,
      partitionKey: { name: 'userId', type: ddb.AttributeType.STRING },
      billingMode: ddb.BillingMode.PAY_PER_REQUEST,
      encryption: ddb.TableEncryption.CUSTOMER_MANAGED,
      encryptionKey: props.kmsKey,
      pointInTimeRecoverySpecification: { pointInTimeRecoveryEnabled: isProd },
      removalPolicy: removal,
    });
    this.userMetaTable.addGlobalSecondaryIndex({
      indexName: 'byStripeCustomer',
      partitionKey: { name: 'stripeCustomerId', type: ddb.AttributeType.STRING },
      projectionType: ddb.ProjectionType.ALL,
    });

    this.pluginRegistryTable = new ddb.Table(this, 'PluginRegistryTable', {
      tableName: `jarvis-${props.envName}-plugins`,
      partitionKey: { name: 'pluginId', type: ddb.AttributeType.STRING },
      sortKey: { name: 'version', type: ddb.AttributeType.STRING },
      billingMode: ddb.BillingMode.PAY_PER_REQUEST,
      encryption: ddb.TableEncryption.CUSTOMER_MANAGED,
      encryptionKey: props.kmsKey,
      removalPolicy: removal,
    });

    // JARVIS AI credits (spec 5.1). All keys are strings; `ts` is an ISO timestamp.
    const aiTable = (id: string, suffix: string, partitionKey: string, sortKey?: string) =>
      new ddb.Table(this, id, {
        tableName: `jarvis-${props.envName}-ai-${suffix}`,
        partitionKey: { name: partitionKey, type: ddb.AttributeType.STRING },
        sortKey: sortKey ? { name: sortKey, type: ddb.AttributeType.STRING } : undefined,
        billingMode: ddb.BillingMode.PAY_PER_REQUEST,
        encryption: ddb.TableEncryption.CUSTOMER_MANAGED,
        encryptionKey: props.kmsKey,
        pointInTimeRecoverySpecification: { pointInTimeRecoveryEnabled: isProd },
        removalPolicy: removal,
        ...(suffix === 'usage' ? { timeToLiveAttribute: 'expiresAt' } : {}),
      });
    const aiUsage = aiTable('AiUsageTable', 'usage', 'userId', 'sk');
    aiUsage.addGlobalSecondaryIndex({
      indexName: 'periodKey-index',
      partitionKey: { name: 'periodKey', type: ddb.AttributeType.STRING },
      sortKey: { name: 'ts', type: ddb.AttributeType.STRING },
      projectionType: ddb.ProjectionType.ALL,
    });
    this.aiTables = {
      usage: aiUsage,
      period: aiTable('AiPeriodTable', 'period', 'userId', 'periodKey'),
      grants: aiTable('AiGrantsTable', 'grants', 'userId', 'grantId'),
      config: aiTable('AiConfigTable', 'config', 'config'),
    };

    new cdk.CfnOutput(this, 'SyncBucketName', {
      value: this.syncBucket.bucketName,
      exportName: `Jarvis-${props.envName}-SyncBucketName`,
    });
    new cdk.CfnOutput(this, 'UserMetaTableName', {
      value: this.userMetaTable.tableName,
      exportName: `Jarvis-${props.envName}-UserMetaTableName`,
    });
    new cdk.CfnOutput(this, 'PluginRegistryTableName', {
      value: this.pluginRegistryTable.tableName,
      exportName: `Jarvis-${props.envName}-PluginRegistryTableName`,
    });
  }
}
