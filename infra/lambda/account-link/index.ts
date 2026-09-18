import type { PreSignUpTriggerEvent } from 'aws-lambda';
import {
  AdminLinkProviderForUserCommand,
  CognitoIdentityProviderClient,
  ListUsersCommand,
} from '@aws-sdk/client-cognito-identity-provider';
import { decide } from './decide';

const cognito = new CognitoIdentityProviderClient({});

export async function handler(event: PreSignUpTriggerEvent): Promise<PreSignUpTriggerEvent> {
  const email = event.request.userAttributes.email;
  if (!email) return event;

  // Quotes and backslashes cannot appear in a valid address; stripping them
  // keeps the filter expression well-formed whatever the provider sends.
  const safeEmail = email.replace(/["\\]/g, '');
  const found = await cognito.send(new ListUsersCommand({
    UserPoolId: event.userPoolId,
    Filter: `email = "${safeEmail}"`,
  }));
  const existing = (found.Users ?? [])
    .filter((u) => u.Username && u.Username !== event.userName)
    .map((u) => ({ username: u.Username!, federated: u.UserStatus === 'EXTERNAL_PROVIDER' }));

  const decision = decide(event.triggerSource, event.userName, existing);
  if (decision.action === 'reject') throw new Error(decision.message);
  if (decision.action === 'link') {
    await cognito.send(new AdminLinkProviderForUserCommand({
      UserPoolId: event.userPoolId,
      DestinationUser: { ProviderName: 'Cognito', ProviderAttributeValue: decision.destinationUsername },
      SourceUser: {
        ProviderName: decision.providerName,
        ProviderAttributeName: 'Cognito_Subject',
        ProviderAttributeValue: decision.providerUserId,
      },
    }));
  }
  return event;
}
