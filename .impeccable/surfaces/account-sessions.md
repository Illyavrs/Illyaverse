# Account login sessions

Mode: Operate.

The original Illyaverse community goal includes usable account management for publicly registered players. Add self-service login revocation to the existing `/settings?tab=account` surface without replacing the website's established design or changing unrelated forms.

## Intended task

A signed-in player can inspect their valid website, stable and lazer logins; revoke one login; or revoke all other logins while keeping the current website session. Current website revocation clearly warns that the player will be signed out and unsaved account settings will not be saved. Other-session revocation confirms that its scope includes all other website sessions and every stable/lazer session, including records omitted from the bounded list.

## Composition and material

Use one compact section under the password form within the incumbent account settings panel, separated by an ordinary horizontal seam. Do not create an additional nested card. The heading, small action buttons, closely grouped client labels and date metadata use the existing fonts and neutral glass palette. Client rows have consistent spacing. Website and game symbols come from the incumbent Lucide system.

The list inherits the account panel's adjustable glass coverage, opacity and blur. The shadcn/Radix AlertDialog uses the floating material tokens so its material remains consistent in dark and light appearance and at every coverage tier. Keep current-login identification textual rather than relying on color. On narrow viewports the row's metadata wraps below its client and action, and translated action labels wrap without horizontal overflow.

## Facts and limits

Only display values supplied by the authenticated sessions API: client type, start time, expiry and actual last activity when present. Do not invent browsers, devices, locations, addresses or online state. Lazer expiry reflects refresh-token validity; a valid login does not prove the player is currently online. Show a direct notice when more than 100 sessions exist. The bulk action still covers records outside the list.

## Required behavior and recovery

- Request only the authenticated player's sessions. Hide this section from guests. Ownership is enforced by the server.
- Revoke a selected noncurrent session immediately; require protected-focus confirmation for current website revocation and bulk revocation.
- Prevent duplicate requests synchronously. A pending row disables its own action; unrelated rows and password fields remain usable. The bulk action and list refresh wait for active row mutations.
- After acknowledged revocation, remove the saved record even if the next list request fails. Explain that the completed sign-out remains valid and provide a list refresh action.
- Handle failed loading, empty lists, unavailable dates, rate limits, already-revoked records and expired website authentication. Never infer a failed server mutation from a missing response.
- On current-session revocation or expired authentication, clear the website's session cache and replace the route with `/login` without issuing a redundant logout request.
- Preserve password form drafts while revoking other logins. Leave profile, appearance and password submission behavior unchanged.
- Confirmation supports keyboard operation, Cancel/Escape, pending protection, visible focus and focus restoration. Opening and closing it must not shift the underlying page horizontally.
- Use bounded API requests and avoid component state updates after unmount.

## Verification handoff

`apps/serendipity/tests/account-sessions.spec.ts` provides intercepted browser fixtures only. The suite covers supplied metadata, individual revocation, duplicate actions, password draft preservation, mutation failures, failed refresh after saved deletion, bounded bulk scope, current-session cache clearing, mobile English confirmation, Japanese empty/error states and keyboard focus restoration. Optional `ILLYA_CAPTURE_ACCOUNT_SESSIONS=1` records the initial combined inspection round; root owns the final-image browser run, finish review and deployment. Real account mutations are not authorized by those fixture tests.

Actual API integration, ownership checks, stable/lazer revocation behavior and final dark/light material evidence must be verified separately against disposable data. Unit or intercepted browser checks alone do not establish those claims.
