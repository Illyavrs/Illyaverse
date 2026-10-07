# Website chat documentation verdict

Verdict: the website chat is an ordinary Operate extension of the incumbent Serendipity system. Preserve `DESIGN.md` and `.impeccable/design.json`; this pass creates only this verdict. No new global primitive token, visual world, artwork or shipping raster is justified by the chat changes.

Role substitution: the dedicated Impeccable documenter is unavailable in the harness catalog; this fresh generic agent followed `reference/degraded/documenter.md` and `reference/document.md` in full.

## Evidence checked

| Authority or source | Evidence used |
| --- | --- |
| `PRODUCT.md` | Pure-black default, neutral adjustable glass, neutral light theme, rounded type, ordinary labels and no AI-generated art. |
| `DESIGN.md` | Normative palette, type and radius frontmatter; Personal Accent, Paired Theme, Bilingual Breathing, Local Overflow, Glass Before Shadow, Independent Material Controls and Shared Radius rules. |
| `.impeccable/design.json` | Existing system narrative, material and breakpoint extensions, and shared control/focus previews. |
| `.impeccable/surfaces/website-chat.md` | THESIS, OWN-WORLD, STORY, FIRST VIEWPORT and FORM for this local Operate extension. |
| `apps/serendipity/src/app/globals.css` | Root/light tokens and focus treatment; chat rules at lines 1648–1942; the mobile rules within the 600px query at lines 2858–3011. |
| `apps/serendipity/src/features/chat.tsx` | Shared controls; translated navigation, errors, preferences and transport status; default collapsed mobile disclosure; labeled transcript and composer; draft and scroll preservation. |
| `apps/serendipity/src/features/chat-data.ts` | Named connection states, separate chat preference state, latest/older history and conversation pagination. |
| `apps/serendipity/src/lib/types.ts` | Peer, unread count, send permission/restriction and `pm_friends_only` data shapes used by the UI. |
| `apps/serendipity/messages/{zh-TW,en,ja}.json` | Equivalent direct labels and explanations for disclosure, private messages, friend-only preference, recovery and transport status. |
| `apps/serendipity/src/components/ui/{button,input,textarea}.tsx` and `src/components/providers.tsx` | Inherited control material, radius, focus/invalid states and the current custom-accent contrast calculation. |

Source after the mobile material/layout fix is the ground truth for this pass. The four capture paths supplied to the finish review are evidence references only; this documenter did not run the app, inspect pixels, rerun the detector or claim a visual/accessibility score:

- `.impeccable/review/website-chat-desktop-dark-zhTW-history.png`
- `.impeccable/review/website-chat-mobile-light-en-create-error.png`
- `.impeccable/review/website-chat-mobile-dark-ja-friend-only.png`
- `.impeccable/review/website-chat-desktop-dark-en-reconnect.png`

## Five-line system summary

1. Palette: black page ground (`#000000`), silver foreground (`#f5f5f5`) and neutral 12/12/12 glass; the runtime personal accent remains authoritative.
2. Paired theme: neutral light ground (`#f5f5f5`), dark text (`#262626`) and white glass use existing semantic replacements; status/error text remains semantic and explicit.
3. Type ramp: Nunito Variable/Noto Sans TC; body 15px/1.7 and 14px on mobile; chat title 17px, message body 14px, name 13px and supporting metadata 10–12px, with wrapping preserved.
4. Named material rules: Personal Accent, Paired Theme, Glass Before Shadow, Independent Material Controls and Shared Radius remain intact; panel blur/opacity and the live 14px-default radius family are reused.
5. Named layout rules: Bilingual Breathing and Local Overflow govern translated controls and bounded scroll regions; the collapsed mobile disclosure and `min(30svh, 220px)` navigation cap remain chat-local rules.

## Surface reconciliation

The desktop source retains navigation beside the transcript and uses the incumbent shared glass container. Public channels and private conversations are separate labeled groups. Private previews, unread counts, the inline recipient field and friend-only preference add account functionality using existing controls and semantic colors. No global material token is added.

At 600px and below, navigation starts collapsed and the toggle names the current selection. Expansion exposes a local vertically scrolling region capped by `--chat-navigation-height`; channel and conversation lists retain local horizontal scrolling. The transcript height subtracts the disclosure/navigation allowance, while header, composer and feedback keep their own space. Selecting a target collapses navigation, restores the toggle focus and preserves per-target scroll position. The source therefore implements the first-viewport intention without promoting its viewport arithmetic into a universal layout system.

Create failures are attached to the recipient field with `aria-invalid`/`aria-describedby`, rendered as alert text and brought into the local navigation viewport. Message, read and preference failures preserve context and provide explicit retry actions. The log is keyboard-focusable, connection text has a status role, and separate polite announcements avoid replaying the whole transcript. These are source observations; the finish review owns rendered behavior verification.

## Not canonized or repaired

Preexisting appearance documentation drift remains: `DESIGN.md` describes the older 0.179 luminance threshold, while the provider now compares dark/light contrast and falls back to black when both are below 4.5; the sidecar's control previews still use raw user-accent outlines while shipped global focus uses readable-accent through `--ring`. Existing universal border-color cascade drift also remains. Chat-specific prose still names a 220px desktop rail and the older mobile arrangement, while current source uses a 260px rail plus disclosure. These facts are reported here because refreshing the authoritative system was not requested; no discrepancy or one-off chat measurement has been legitimized as a new global rule.
