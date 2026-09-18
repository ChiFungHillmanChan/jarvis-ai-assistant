import * as cdk from 'aws-cdk-lib';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as kms from 'aws-cdk-lib/aws-kms';
import * as iam from 'aws-cdk-lib/aws-iam';
import * as lambda from 'aws-cdk-lib/aws-lambda';
import * as nodejs from 'aws-cdk-lib/aws-lambda-nodejs';
import * as sm from 'aws-cdk-lib/aws-secretsmanager';
import { Construct } from 'constructs';
import * as path from 'path';

export interface AuthStackProps extends cdk.StackProps {
  envName: string;
  /** Where Cognito sends the browser after sign-out. */
  appDomain: string;
}

export class AuthStack extends cdk.Stack {
  public readonly userPool: cognito.UserPool;
  public readonly userPoolClient: cognito.UserPoolClient;
  public readonly dataKey: kms.Key;
  public readonly domain: cognito.UserPoolDomain;

  constructor(scope: Construct, id: string, props: AuthStackProps) {
    super(scope, id, props);

    const isProd = props.envName === 'prod';

    this.dataKey = new kms.Key(this, 'JarvisDataKey', {
      alias: `alias/jarvis/${props.envName}/data`,
      description: `Customer-managed CMK for JARVIS ${props.envName} envelope encryption`,
      enableKeyRotation: true,
      removalPolicy: isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY,
    });

    // One account per verified email across password and Google sign-in.
    const accountLinkFn = new nodejs.NodejsFunction(this, 'AccountLinkFn', {
      functionName: `jarvis-${props.envName}-account-link`,
      runtime: lambda.Runtime.NODEJS_24_X,
      entry: path.join(__dirname, '../lambda/account-link/index.ts'),
      handler: 'handler',
      timeout: cdk.Duration.seconds(5),
      memorySize: 256,
      bundling: { minify: true, sourceMap: true },
    });
    // Built from the stack, not from the pool: referencing userPool.userPoolArn
    // here makes the pool depend on the function and the function's policy
    // depend on the pool, which CloudFormation rejects as a cycle. The handler
    // only ever acts on event.userPoolId, the pool that invoked it.
    accountLinkFn.addToRolePolicy(new iam.PolicyStatement({
      actions: ['cognito-idp:ListUsers', 'cognito-idp:AdminLinkProviderForUser'],
      resources: [cdk.Stack.of(this).formatArn({ service: 'cognito-idp', resource: 'userpool', resourceName: '*' })],
    }));

    this.userPool = new cognito.UserPool(this, 'JarvisUserPool', {
      userPoolName: `jarvis-${props.envName}-users`,
      signInAliases: { email: true },
      selfSignUpEnabled: true,
      autoVerify: { email: true },
      passwordPolicy: {
        minLength: 12,
        requireLowercase: true,
        requireUppercase: true,
        requireDigits: true,
        requireSymbols: false,
      },
      mfa: cognito.Mfa.OPTIONAL,
      mfaSecondFactor: { sms: false, otp: true },
      accountRecovery: cognito.AccountRecovery.EMAIL_ONLY,
      standardAttributes: {
        email: { required: true, mutable: false },
        givenName: { required: false, mutable: true },
      },
      customAttributes: {
        tier: new cognito.StringAttribute({ minLen: 1, maxLen: 20, mutable: true }),
        stripeCustomerId: new cognito.StringAttribute({ minLen: 1, maxLen: 64, mutable: true }),
      },
      lambdaTriggers: { preSignUp: accountLinkFn },
      removalPolicy: isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY,
    });

    const googleSecret = sm.Secret.fromSecretNameV2(this, 'GoogleLoginSecret', `jarvis/${props.envName}/google-login`);
    const google = new cognito.UserPoolIdentityProviderGoogle(this, 'Google', {
      userPool: this.userPool,
      // Not a secret, but kept next to the secret so both are rotated together
      // and neither is ever committed.
      clientId: googleSecret.secretValueFromJson('clientId').unsafeUnwrap(),
      clientSecretValue: googleSecret.secretValueFromJson('clientSecret'),
      scopes: ['openid', 'email', 'profile'],
      attributeMapping: {
        email: cognito.ProviderAttribute.GOOGLE_EMAIL,
        emailVerified: cognito.ProviderAttribute.GOOGLE_EMAIL_VERIFIED,
        givenName: cognito.ProviderAttribute.GOOGLE_GIVEN_NAME,
      },
    });

    // Must match LOOPBACK_PORTS in src-tauri/src/auth/cognito.rs exactly.
    const loopbackPorts = [47821, 47822, 47823];
    this.userPoolClient = this.userPool.addClient('JarvisAppClient', {
      userPoolClientName: `jarvis-${props.envName}-app`,
      authFlows: {
        userSrp: true,
        userPassword: false,
        adminUserPassword: false,
        custom: false,
      },
      preventUserExistenceErrors: true,
      enableTokenRevocation: true,
      refreshTokenValidity: cdk.Duration.days(30),
      accessTokenValidity: cdk.Duration.hours(1),
      idTokenValidity: cdk.Duration.hours(1),
      supportedIdentityProviders: [
        cognito.UserPoolClientIdentityProvider.COGNITO,
        cognito.UserPoolClientIdentityProvider.GOOGLE,
      ],
      oAuth: {
        flows: { authorizationCodeGrant: true, implicitCodeGrant: false },
        scopes: [cognito.OAuthScope.OPENID, cognito.OAuthScope.EMAIL, cognito.OAuthScope.PROFILE],
        callbackUrls: loopbackPorts.map((port) => `http://127.0.0.1:${port}/callback`),
        logoutUrls: [`https://${props.appDomain}/`],
      },
    });
    this.userPoolClient.node.addDependency(google);

    this.domain = this.userPool.addDomain('JarvisDomain', {
      cognitoDomain: { domainPrefix: `jarvis-automation-${props.envName}` },
      managedLoginVersion: cognito.ManagedLoginVersion.NEWER_MANAGED_LOGIN,
    });
    // Managed login (v2) renders nothing for a client without a branding style.
    new cognito.CfnManagedLoginBranding(this, 'JarvisLoginBranding', {
      userPoolId: this.userPool.userPoolId,
      clientId: this.userPoolClient.userPoolClientId,
      useCognitoProvidedValues: true,
    });

    new cdk.CfnOutput(this, 'UserPoolId', {
      value: this.userPool.userPoolId,
      exportName: `Jarvis-${props.envName}-UserPoolId`,
    });
    new cdk.CfnOutput(this, 'UserPoolClientId', {
      value: this.userPoolClient.userPoolClientId,
      exportName: `Jarvis-${props.envName}-UserPoolClientId`,
    });
    new cdk.CfnOutput(this, 'DataKeyArn', {
      value: this.dataKey.keyArn,
      exportName: `Jarvis-${props.envName}-DataKeyArn`,
    });
    new cdk.CfnOutput(this, 'UserPoolDomainUrl', {
      value: this.domain.baseUrl(),
      exportName: `Jarvis-${props.envName}-UserPoolDomainUrl`,
    });
    new cdk.CfnOutput(this, 'UserPoolArn', {
      value: this.userPool.userPoolArn,
      exportName: `Jarvis-${props.envName}-UserPoolArn`,
    });
  }
}
