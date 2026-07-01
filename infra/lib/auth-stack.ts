import * as cdk from 'aws-cdk-lib';
import * as cognito from 'aws-cdk-lib/aws-cognito';
import * as kms from 'aws-cdk-lib/aws-kms';
import { Construct } from 'constructs';

export interface AuthStackProps extends cdk.StackProps {
  envName: string;
}

export class AuthStack extends cdk.Stack {
  public readonly userPool: cognito.UserPool;
  public readonly userPoolClient: cognito.UserPoolClient;
  public readonly dataKey: kms.Key;

  constructor(scope: Construct, id: string, props: AuthStackProps) {
    super(scope, id, props);

    const isProd = props.envName === 'prod';

    this.dataKey = new kms.Key(this, 'JarvisDataKey', {
      alias: `alias/jarvis/${props.envName}/data`,
      description: `Customer-managed CMK for JARVIS ${props.envName} envelope encryption`,
      enableKeyRotation: true,
      removalPolicy: isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY,
    });

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
      removalPolicy: isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY,
    });

    this.userPoolClient = this.userPool.addClient('JarvisAppClient', {
      userPoolClientName: `jarvis-${props.envName}-app`,
      authFlows: {
        userSrp: true,
        userPassword: false,
        adminUserPassword: false,
        custom: false,
      },
      preventUserExistenceErrors: true,
      refreshTokenValidity: cdk.Duration.days(30),
      accessTokenValidity: cdk.Duration.hours(1),
      idTokenValidity: cdk.Duration.hours(1),
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
  }
}
