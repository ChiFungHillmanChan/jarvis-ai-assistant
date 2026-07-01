import * as cdk from 'aws-cdk-lib';
import * as cw from 'aws-cdk-lib/aws-cloudwatch';
import * as budgets from 'aws-cdk-lib/aws-budgets';
import * as sns from 'aws-cdk-lib/aws-sns';
import * as snsSubs from 'aws-cdk-lib/aws-sns-subscriptions';
import { Construct } from 'constructs';

export interface ObservabilityStackProps extends cdk.StackProps {
  envName: string;
  billingAlertEmail: string;
  syncBucketName: string;
}

export class ObservabilityStack extends cdk.Stack {
  constructor(scope: Construct, id: string, props: ObservabilityStackProps) {
    super(scope, id, props);

    const alertTopic = new sns.Topic(this, 'JarvisAlertTopic', {
      topicName: `jarvis-${props.envName}-alerts`,
      displayName: `JARVIS ${props.envName} alerts`,
    });
    alertTopic.addSubscription(new snsSubs.EmailSubscription(props.billingAlertEmail));

    const thresholds = props.envName === 'prod' ? [50, 100, 500] : [5, 10, 25];
    for (const limit of thresholds) {
      new budgets.CfnBudget(this, `Budget${limit}USD`, {
        budget: {
          budgetName: `jarvis-${props.envName}-budget-${limit}usd`,
          budgetLimit: { amount: limit, unit: 'USD' },
          timeUnit: 'MONTHLY',
          budgetType: 'COST',
          costFilters: { TagKeyValue: [`user:Project$jarvis`] },
        },
        notificationsWithSubscribers: [
          {
            notification: {
              notificationType: 'ACTUAL',
              comparisonOperator: 'GREATER_THAN',
              threshold: 50,
              thresholdType: 'PERCENTAGE',
            },
            subscribers: [
              {
                subscriptionType: 'EMAIL',
                address: props.billingAlertEmail,
              },
            ],
          },
          {
            notification: {
              notificationType: 'ACTUAL',
              comparisonOperator: 'GREATER_THAN',
              threshold: 100,
              thresholdType: 'PERCENTAGE',
            },
            subscribers: [
              {
                subscriptionType: 'EMAIL',
                address: props.billingAlertEmail,
              },
            ],
          },
          {
            notification: {
              notificationType: 'FORECASTED',
              comparisonOperator: 'GREATER_THAN',
              threshold: 100,
              thresholdType: 'PERCENTAGE',
            },
            subscribers: [
              {
                subscriptionType: 'EMAIL',
                address: props.billingAlertEmail,
              },
            ],
          },
        ],
      });
    }

    const dashboard = new cw.Dashboard(this, 'JarvisDashboard', {
      dashboardName: `jarvis-${props.envName}`,
      defaultInterval: cdk.Duration.hours(6),
    });

    dashboard.addWidgets(
      new cw.TextWidget({
        markdown: `# JARVIS ${props.envName.toUpperCase()} dashboard\n\nUsed for HKSTP Gold Badge Market Validation Report (Q2 2026).`,
        width: 24,
        height: 2,
      })
    );

    // AWS/Billing EstimatedCharges metric is only published in us-east-1.
    // We rely on AWS Budgets (above) for cross-region cost alerts; budgets
    // notify at 50%, 100% actual and 100% forecasted of each threshold.

    new cdk.CfnOutput(this, 'AlertTopicArn', {
      value: alertTopic.topicArn,
      exportName: `Jarvis-${props.envName}-AlertTopicArn`,
    });
    new cdk.CfnOutput(this, 'DashboardName', {
      value: dashboard.dashboardName,
      exportName: `Jarvis-${props.envName}-DashboardName`,
    });
  }
}
