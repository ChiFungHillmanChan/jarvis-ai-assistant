// One JARVIS account per verified email across the two login methods.
// Without this, a user who registered with a password and later chooses
// "Continue with Google" gets a second Cognito user with a different sub,
// and their Pro subscription (keyed by sub) appears to vanish.

export interface ExistingUser {
  username: string;
  /** Cognito reports federated users with UserStatus EXTERNAL_PROVIDER. */
  federated: boolean;
}

export type LinkDecision =
  | { action: 'allow' }
  | { action: 'link'; destinationUsername: string; providerName: string; providerUserId: string }
  | { action: 'reject'; message: string };

/** The identity provider name configured in infra/lib/auth-stack.ts. */
const GOOGLE = 'Google';

export function decide(triggerSource: string, userName: string, existing: ExistingUser[]): LinkDecision {
  if (triggerSource === 'PreSignUp_ExternalProvider') {
    const separator = userName.indexOf('_');
    const prefix = separator > 0 ? userName.slice(0, separator) : '';
    if (prefix.toLowerCase() !== GOOGLE.toLowerCase()) return { action: 'allow' };
    const nativeUser = existing.find((u) => !u.federated);
    if (!nativeUser) return { action: 'allow' };
    return {
      action: 'link',
      destinationUsername: nativeUser.username,
      providerName: GOOGLE,
      providerUserId: userName.slice(separator + 1),
    };
  }
  if (triggerSource === 'PreSignUp_SignUp' && existing.some((u) => u.federated)) {
    return { action: 'reject', message: 'This email uses Google sign-in. Choose Continue with Google.' };
  }
  return { action: 'allow' };
}
