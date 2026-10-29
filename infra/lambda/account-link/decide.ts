// One JARVIS account per verified email across the two login methods.
// Without this, a user who registered with a password and later chooses
// "Continue with Google" gets a second Cognito user with a different sub,
// and their Pro subscription (keyed by sub) appears to vanish. Linking
// requires both sides to be verified, or a stranger could pre-register an
// unconfirmed account against someone else's address and either capture the
// real owner's later Google sign-in or block their email sign-up outright.

export interface ExistingUser {
  username: string;
  /** Cognito reports federated users with UserStatus EXTERNAL_PROVIDER. */
  federated: boolean;
  /**
   * A native user counts as verified only once UserStatus is CONFIRMED and
   * email_verified is "true"; a federated user counts as verified once
   * email_verified is "true".
   */
  verified: boolean;
}

export type LinkDecision =
  | { action: 'allow' }
  | { action: 'link'; destinationUsername: string; providerName: string; providerUserId: string }
  | { action: 'reject'; message: string };

export const CUSTOM_ATTRIBUTES_REFUSED = 'Custom attributes cannot be set at sign-up.';

/**
 * The app client keeps Cognito's default write attributes, so the unauthenticated SignUp API would
 * accept custom:* attributes. Nothing legitimate sends them at sign-up; refuse every one.
 */
export function hasCustomAttribute(attributeNames: string[]): boolean {
  return attributeNames.some((name) => name.startsWith('custom:'));
}

/** The identity provider name configured in infra/lib/auth-stack.ts. */
const GOOGLE = 'Google';

export function decide(
  triggerSource: string,
  userName: string,
  emailVerified: boolean,
  existing: ExistingUser[],
): LinkDecision {
  if (triggerSource === 'PreSignUp_ExternalProvider') {
    const separator = userName.indexOf('_');
    const prefix = separator > 0 ? userName.slice(0, separator) : '';
    if (prefix.toLowerCase() !== GOOGLE.toLowerCase()) return { action: 'allow' };
    if (!emailVerified) return { action: 'allow' };
    const nativeUser = existing.find((u) => !u.federated && u.verified);
    if (!nativeUser) return { action: 'allow' };
    return {
      action: 'link',
      destinationUsername: nativeUser.username,
      providerName: GOOGLE,
      providerUserId: userName.slice(separator + 1),
    };
  }
  if (triggerSource === 'PreSignUp_SignUp' && existing.some((u) => u.federated && u.verified)) {
    return { action: 'reject', message: 'This email uses Google sign-in. Choose Continue with Google.' };
  }
  return { action: 'allow' };
}
