import * as cdk from 'aws-cdk-lib';
import * as apigwv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as apigwv2Integ from 'aws-cdk-lib/aws-apigatewayv2-integrations';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as nodejs from 'aws-cdk-lib/aws-lambda-nodejs';
import * as s3 from 'aws-cdk-lib/aws-s3';
import { Construct } from 'constructs';
import * as path from 'path';

export interface RegistryStackProps extends cdk.StackProps {
  envName: string;
  /** owner/name of the GitHub repository whose Registry workflow may publish. */
  githubRepo: string;
}

/** The objects the publisher may write and the read Lambda serves. */
export const REGISTRY_PREFIX = 'registry/';
export const LATEST_KEY = 'registry/latest.json';

/**
 * The signed model registry (registry/ at the repository root).
 *
 * - A private, versioned bucket holding registry/v<version>.json(.sig) and
 *   registry/latest.json(.sig). Only the Registry workflow writes it, through
 *   a GitHub OIDC role limited to registry/* and to this env's GitHub
 *   environment (registry-<env>).
 * - GET /v1/registry on the existing control-plane HTTP API (Payment stack),
 *   public, served by a Lambda that may read registry/latest.json only.
 *
 * The registry is public by design: it must never hold a key, a secret or any
 * user data. registry/validate.ts refuses strings that look like either.
 *
 * Its own stack, not the Data stack: the route lives on the Payment stack's
 * API, and the Payment stack already depends on Data, so a route added from
 * Data would make a cycle. Keeping CI-writable, public content apart from the
 * user-data stacks also keeps the publisher role's blast radius obvious.
 */
export class RegistryStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: RegistryStackProps) {
    super(scope, id, props);
    if (!/^[A-Za-z0-9-]+\/[A-Za-z0-9._-]+$/.test(props.githubRepo)) {
      throw new Error(`githubRepo must be owner/name, got "${props.githubRepo}"`);
    }
    const isProd = props.envName === 'prod';

    const bucket = new s3.Bucket(this, 'RegistryBucket', {
      bucketName: `jarvis-registry-${props.envName}-${this.account}`,
      encryption: s3.BucketEncryption.S3_MANAGED,
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      objectOwnership: s3.ObjectOwnership.BUCKET_OWNER_ENFORCED,
      enforceSSL: true,
      // Every published version stays readable for rollback and audit; the
      // noncurrent copies of latest.json expire after a year.
      versioned: true,
      lifecycleRules: [{ id: 'expire-old-latest', noncurrentVersionExpiration: cdk.Duration.days(365) }],
      removalPolicy: isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY,
      autoDeleteObjects: !isProd,
    });

    // The account's GitHub OIDC provider. IAM allows one per URL per account,
    // and dev and prod share the account, so it is created once outside CDK
    // (registry/README.md, first-time setup) and referenced here.
    const githubOidc = iam.OpenIdConnectProvider.fromOpenIdConnectProviderArn(
      this, 'GithubOidc', `arn:${this.partition}:iam::${this.account}:oidc-provider/token.actions.githubusercontent.com`,
    );
    const publisher = new iam.Role(this, 'RegistryPublisherRole', {
      roleName: `jarvis-${props.envName}-registry-publisher`,
      description: `Registry workflow (${props.githubRepo}, GitHub environment registry-${props.envName}) writes ${REGISTRY_PREFIX}*`,
      maxSessionDuration: cdk.Duration.hours(1),
      assumedBy: new iam.WebIdentityPrincipal(githubOidc.openIdConnectProviderArn, {
        StringEquals: {
          'token.actions.githubusercontent.com:aud': 'sts.amazonaws.com',
          'token.actions.githubusercontent.com:sub': `repo:${props.githubRepo}:environment:registry-${props.envName}`,
        },
      }),
    });
    publisher.addToPolicy(new iam.PolicyStatement({
      actions: ['s3:GetObject', 's3:PutObject'],
      resources: [bucket.arnForObjects(`${REGISTRY_PREFIX}*`)],
    }));
    publisher.addToPolicy(new iam.PolicyStatement({
      actions: ['s3:ListBucket'],
      resources: [bucket.bucketArn],
      conditions: { StringLike: { 's3:prefix': [`${REGISTRY_PREFIX}*`] } },
    }));

    const readFn = new nodejs.NodejsFunction(this, 'RegistryReadFn', {
      functionName: `jarvis-${props.envName}-registry-read`,
      runtime: lambda.Runtime.NODEJS_24_X,
      entry: path.join(__dirname, '../lambda/registry/index.ts'),
      handler: 'handler',
      timeout: cdk.Duration.seconds(5),
      memorySize: 256,
      environment: { REGISTRY_BUCKET: bucket.bucketName },
      bundling: { minify: true, sourceMap: true },
    });
    readFn.addToRolePolicy(new iam.PolicyStatement({
      actions: ['s3:GetObject'],
      resources: [bucket.arnForObjects(LATEST_KEY)],
    }));

    // The control-plane HTTP API, read from the Payment stack's existing
    // ApiBaseUrl export (https://<api-id>.execute-api.<region>.amazonaws.com),
    // so the Payment stack's template does not change. While this stack
    // exists, CloudFormation refuses to change or delete that export. The
    // route inherits the API's default stage throttle (10 requests a second,
    // burst 20).
    const apiBaseUrl = cdk.Fn.importValue(`Jarvis-${props.envName}-ApiBaseUrl`);
    const httpApiId = cdk.Fn.select(0, cdk.Fn.split('.', cdk.Fn.select(1, cdk.Fn.split('//', apiBaseUrl))));
    const httpApi = apigwv2.HttpApi.fromHttpApiAttributes(this, 'ControlPlaneApi', { httpApiId });
    new apigwv2.HttpRoute(this, 'RegistryRoute', {
      httpApi,
      routeKey: apigwv2.HttpRouteKey.with('/v1/registry', apigwv2.HttpMethod.GET),
      integration: new apigwv2Integ.HttpLambdaIntegration('RegistryIntegration', readFn),
    });

    new cdk.CfnOutput(this, 'RegistryBucketName', {
      value: bucket.bucketName,
      description: 'GitHub environment variable REGISTRY_BUCKET for registry-' + props.envName,
    });
    new cdk.CfnOutput(this, 'RegistryPublisherRoleArn', {
      value: publisher.roleArn,
      description: 'GitHub environment variable REGISTRY_ROLE_ARN for registry-' + props.envName,
    });
    new cdk.CfnOutput(this, 'RegistryUrl', {
      value: cdk.Fn.join('', [apiBaseUrl, '/v1/registry']),
      description: 'GET the signed registry here',
    });
  }
}
