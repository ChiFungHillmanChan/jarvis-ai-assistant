import * as cdk from 'aws-cdk-lib';
import * as apigwv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as apigwv2Integ from 'aws-cdk-lib/aws-apigatewayv2-integrations';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as nodejs from 'aws-cdk-lib/aws-lambda-nodejs';
import * as sm from 'aws-cdk-lib/aws-secretsmanager';
import * as ddb from 'aws-cdk-lib/aws-dynamodb';
import { HttpJwtAuthorizer } from 'aws-cdk-lib/aws-apigatewayv2-authorizers';
import { Construct } from 'constructs';
import * as path from 'path';

export interface PaymentStackProps extends cdk.StackProps {
  envName: string;
  userMetaTable: ddb.ITable;
  userPool: cognito.IUserPool;
  userPoolClient: cognito.IUserPoolClient;
}

export class PaymentStack extends cdk.Stack {
  public readonly webhookUrl: string;

  constructor(scope: Construct, id: string, props: PaymentStackProps) {
    super(scope, id, props);

    const stripeSecrets = new sm.Secret(this, 'StripeSecrets', {
      secretName: `jarvis/${props.envName}/stripe`,
      description: 'Stripe webhook signing secret + restricted API key',
      generateSecretString: {
        secretStringTemplate: JSON.stringify({
          webhookSecret: 'PLACEHOLDER_whsec_xxx',
          apiKey: 'PLACEHOLDER_sk_test_xxx',
        }),
        generateStringKey: 'placeholder',
      },
    });

    const webhookFn = new nodejs.NodejsFunction(this, 'StripeWebhookFn', {
      functionName: `jarvis-${props.envName}-stripe-webhook`,
      runtime: lambda.Runtime.NODEJS_24_X,
      entry: path.join(__dirname, '../lambda/stripe-webhook/index.ts'),
      handler: 'handler',
      timeout: cdk.Duration.seconds(10),
      memorySize: 256,
      environment: {
        STRIPE_SECRET_NAME: stripeSecrets.secretName,
        USER_META_TABLE: props.userMetaTable.tableName,
        ENV: props.envName,
      },
      bundling: { minify: true, sourceMap: true },
    });
    stripeSecrets.grantRead(webhookFn);
    props.userMetaTable.grantReadWriteData(webhookFn);

    const checkoutFn = new nodejs.NodejsFunction(this, 'StripeCheckoutFn', {
      functionName: `jarvis-${props.envName}-stripe-checkout`,
      runtime: lambda.Runtime.NODEJS_24_X,
      entry: path.join(__dirname, '../lambda/stripe-checkout/index.ts'),
      handler: 'handler',
      timeout: cdk.Duration.seconds(10),
      memorySize: 256,
      environment: {
        STRIPE_SECRET_NAME: stripeSecrets.secretName,
        USER_META_TABLE: props.userMetaTable.tableName,
        ENV: props.envName,
        APP_DOMAIN: 'jarvis-automation.com',
        USER_POOL_ID: props.userPool.userPoolId,
      },
      bundling: { minify: true, sourceMap: true },
    });
    stripeSecrets.grantRead(checkoutFn);
    props.userMetaTable.grantReadWriteData(checkoutFn);

    const meFn = new nodejs.NodejsFunction(this, 'MeFn', {
      functionName: `jarvis-${props.envName}-me`,
      runtime: lambda.Runtime.NODEJS_24_X,
      entry: path.join(__dirname, '../lambda/me/index.ts'),
      handler: 'handler',
      timeout: cdk.Duration.seconds(5),
      memorySize: 256,
      environment: {
        USER_META_TABLE: props.userMetaTable.tableName,
        USER_POOL_ID: props.userPool.userPoolId,
      },
      bundling: { minify: true, sourceMap: true },
    });
    props.userMetaTable.grantReadData(meFn);

    // Both look up the signed-in user's email; the access token does not carry it.
    const adminGetUser = new iam.PolicyStatement({
      actions: ['cognito-idp:AdminGetUser'],
      resources: [props.userPool.userPoolArn],
    });
    meFn.addToRolePolicy(adminGetUser);
    checkoutFn.addToRolePolicy(adminGetUser);

    const authorizer = new HttpJwtAuthorizer(
      'CognitoAuthorizer',
      `https://cognito-idp.${this.region}.amazonaws.com/${props.userPool.userPoolId}`,
      {
        jwtAudience: [props.userPoolClient.userPoolClientId],
        identitySource: ['$request.header.Authorization'],
      },
    );
    // Cognito access tokens carry a scope claim and ID tokens do not, so
    // requiring `openid` stops an ID token being used as an access token.
    const authorised = { authorizer, authorizationScopes: ['openid'] };

    const httpApi = new apigwv2.HttpApi(this, 'PaymentApi', {
      apiName: `jarvis-${props.envName}-payments`,
      description: 'JARVIS control-plane API (payments, account)',
      corsPreflight: undefined,
    });
    httpApi.addRoutes({
      path: '/stripe/webhook',
      methods: [apigwv2.HttpMethod.POST],
      integration: new apigwv2Integ.HttpLambdaIntegration('WebhookIntegration', webhookFn),
    });
    httpApi.addRoutes({
      path: '/checkout',
      methods: [apigwv2.HttpMethod.POST],
      integration: new apigwv2Integ.HttpLambdaIntegration('CheckoutIntegration', checkoutFn),
      ...authorised,
    });
    httpApi.addRoutes({
      path: '/me',
      methods: [apigwv2.HttpMethod.GET],
      integration: new apigwv2Integ.HttpLambdaIntegration('MeIntegration', meFn),
      ...authorised,
    });

    // POST /checkout and GET /me require a Cognito access token (JWT authorizer
    // above); the Lambdas take the user id from the verified `sub` claim.
    // Throttling stays as a second line: API Gateway throttles per stage and
    // route, so it caps cost, not any one caller.
    const defaultStage = httpApi.defaultStage!.node.defaultChild as apigwv2.CfnStage;
    defaultStage.defaultRouteSettings = {
      throttlingRateLimit: 10,
      throttlingBurstLimit: 20,
    };
    defaultStage.routeSettings = {
      // Real usage is single-digit requests per minute: one click per upgrade.
      'POST /checkout': {
        throttlingRateLimit: 2,
        throttlingBurstLimit: 5,
      },
    };

    this.webhookUrl = `${httpApi.apiEndpoint}/stripe/webhook`;
    const checkoutUrl = `${httpApi.apiEndpoint}/checkout`;

    new cdk.CfnOutput(this, 'StripeWebhookUrl', {
      value: this.webhookUrl,
      description: 'Configure this URL in Stripe Dashboard -> Developers -> Webhooks',
      exportName: `Jarvis-${props.envName}-StripeWebhookUrl`,
    });
    new cdk.CfnOutput(this, 'CheckoutUrl', {
      value: checkoutUrl,
      description: 'POST {tier} here with a Cognito access token to start a Stripe Checkout',
      exportName: `Jarvis-${props.envName}-CheckoutUrl`,
    });
    new cdk.CfnOutput(this, 'StripeSecretArn', {
      value: stripeSecrets.secretArn,
      exportName: `Jarvis-${props.envName}-StripeSecretArn`,
    });
    new cdk.CfnOutput(this, 'ApiBaseUrl', {
      value: httpApi.apiEndpoint,
      exportName: `Jarvis-${props.envName}-ApiBaseUrl`,
    });
  }
}
