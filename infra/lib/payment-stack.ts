import * as cdk from 'aws-cdk-lib';
import * as apigwv2 from 'aws-cdk-lib/aws-apigatewayv2';
import * as apigwv2Integ from 'aws-cdk-lib/aws-apigatewayv2-integrations';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as nodejs from 'aws-cdk-lib/aws-lambda-nodejs';
import * as sm from 'aws-cdk-lib/aws-secretsmanager';
import * as ddb from 'aws-cdk-lib/aws-dynamodb';
import { Construct } from 'constructs';
import * as path from 'path';

export interface PaymentStackProps extends cdk.StackProps {
  envName: string;
  userMetaTable: ddb.ITable;
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
      runtime: lambda.Runtime.NODEJS_20_X,
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
      runtime: lambda.Runtime.NODEJS_20_X,
      entry: path.join(__dirname, '../lambda/stripe-checkout/index.ts'),
      handler: 'handler',
      timeout: cdk.Duration.seconds(10),
      memorySize: 256,
      environment: {
        STRIPE_SECRET_NAME: stripeSecrets.secretName,
        USER_META_TABLE: props.userMetaTable.tableName,
        ENV: props.envName,
        APP_DOMAIN: 'jarvis-automation.com',
      },
      bundling: { minify: true, sourceMap: true },
    });
    stripeSecrets.grantRead(checkoutFn);
    props.userMetaTable.grantReadWriteData(checkoutFn);

    const httpApi = new apigwv2.HttpApi(this, 'PaymentApi', {
      apiName: `jarvis-${props.envName}-payments`,
      description: 'Stripe webhook endpoint',
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
    });

    // STOPGAP, NOT AUTHENTICATION. POST /checkout has no authorizer: the caller
    // asserts its own user_id and email and nothing verifies either, so anyone
    // holding the URL can make the Lambda create Stripe customers and Checkout
    // Sessions. Throttling caps the blast radius; it does not establish identity,
    // and because API Gateway throttles per stage/route rather than per caller, a
    // flood still degrades real users.
    // An authorizer is REQUIRED before this endpoint sees a live Stripe key. The
    // intended one is the Cognito user pool AuthStack already provisions — see the
    // note on resolve_install_id in src-tauri/src/commands/payments.rs about the
    // Cognito subject replacing install_id. Not wired here because the desktop
    // client has no Cognito integration yet and would break on day one.
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
      description: 'POST {tier, user_id, email} here to start a Stripe Checkout',
      exportName: `Jarvis-${props.envName}-CheckoutUrl`,
    });
    new cdk.CfnOutput(this, 'StripeSecretArn', {
      value: stripeSecrets.secretArn,
      exportName: `Jarvis-${props.envName}-StripeSecretArn`,
    });
  }
}
