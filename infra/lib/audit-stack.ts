import * as cdk from 'aws-cdk-lib';
import * as s3 from 'aws-cdk-lib/aws-s3';
import * as ct from 'aws-cdk-lib/aws-cloudtrail';
import * as kms from 'aws-cdk-lib/aws-kms';
import { Construct } from 'constructs';

export interface AuditStackProps extends cdk.StackProps {
  envName: string;
  kmsKey: kms.IKey;
}

export class AuditStack extends cdk.Stack {
  public readonly trail: ct.Trail;

  constructor(scope: Construct, id: string, props: AuditStackProps) {
    super(scope, id, props);

    const isProd = props.envName === 'prod';
    const removal = isProd ? cdk.RemovalPolicy.RETAIN : cdk.RemovalPolicy.DESTROY;

    const logsBucket = new s3.Bucket(this, 'CloudTrailLogsBucket', {
      bucketName: `jarvis-cloudtrail-${props.envName}-${this.account}`,
      encryption: s3.BucketEncryption.S3_MANAGED,
      blockPublicAccess: s3.BlockPublicAccess.BLOCK_ALL,
      enforceSSL: true,
      versioned: false,
      lifecycleRules: [
        {
          id: 'expire-after-90-days',
          enabled: true,
          expiration: cdk.Duration.days(isProd ? 365 : 90),
          noncurrentVersionExpiration: cdk.Duration.days(7),
        },
      ],
      removalPolicy: removal,
      autoDeleteObjects: !isProd,
    });

    this.trail = new ct.Trail(this, 'JarvisTrail', {
      trailName: `jarvis-${props.envName}-trail`,
      bucket: logsBucket,
      isMultiRegionTrail: true,
      includeGlobalServiceEvents: true,
      enableFileValidation: true,
      managementEvents: ct.ReadWriteType.ALL,
    });

    new cdk.CfnOutput(this, 'TrailArn', {
      value: this.trail.trailArn,
      exportName: `Jarvis-${props.envName}-TrailArn`,
    });
    new cdk.CfnOutput(this, 'CloudTrailBucketName', {
      value: logsBucket.bucketName,
      exportName: `Jarvis-${props.envName}-CloudTrailBucketName`,
    });
  }
}
