# CrowClaw violet branding design QA

Date: 2026-07-17

Branch: `codex/violet-branding`

Base: `ab3f6fac0ba527a1bca84d820409417a21f67acb`

## Design intent and source truth

- Preserve the accepted CrowClaw desktop layout and every existing interaction.
- Move the interface from black/orange to deep black, ultraviolet, violet and magenta, with restrained cyan for selected identity accents.
- Use Crow's supplied cybernetic crow artwork as the real product identity; do not replace it with CSS art or a fabricated vector.
- Treat the supplied `0807faee-7e1d-4941-9380-3dfead5bf273.png` brand board as the palette and visual-density reference.
- Use the supplied `574004261-d3723026-4bfb-48c4-93bb-2453d5d582d2.png` artwork as the primary in-product emblem.

## Comparison evidence

The source brand board and the 1180x760 onboarding capture were inspected together in one comparison pass. Additional local captures covered:

- onboarding at 1180x760;
- populated and empty chat at 1180x760;
- CrowQuant Memory at 1180x760;
- approval dialog at 1180x760;
- populated chat while the native viewport was set to the 920x640 minimum.

The ignored local evidence directory is `.test-runtime/design-qa/`. Its captures are qualitative visual evidence; viewport and overflow claims below come from recorded DOM geometry, not image pixel dimensions.

## Mandatory QA passes

### Fidelity, typography, spacing and layout

- Existing layout hierarchy, navigation, forms, composer, dialogs, cards and responsive grid behavior remain intact.
- The onboarding hero uses a purpose-cropped 940x458 derivative; deliberately square identity slots use 96x96 and 256x256 derivatives of the same supplied emblem.
- The default 1180x760 onboarding shell has no vertical or horizontal overflow after the final spacing correction.
- With the native viewport set to 920x640, DOM geometry reports a 920x640 shell with no document or application overflow; the composer remains entirely reachable.
- The app continues to use its established Segoe UI Variable/Cascadia Mono typography system.

### Colors, states and accessibility

- Primary surfaces use `#05030a`, `#0a0710`, `#110c1b`, `#181124` and `#211832`.
- Primary identity accents use violet `#9d5cff`, bright violet `#d873ff`, magenta `#ff4fd8` and restrained cyan `#5be3ff`.
- Muted text was raised to `#9589a6`; this clears the prior P1 contrast failure on the strongest surface.
- The primary-button gradient dark stop was raised to `#8067ff`; dark button text now clears the prior P1 contrast failure.
- Warning, danger, success and approval semantics retain distinct amber, red and green treatments instead of being flattened into the brand hue.
- Active navigation uses color plus an inset shape marker; focus-visible controls retain a two-pixel outline; the approval capture visibly confirms keyboard focus.
- Existing reduced-motion handling remains unchanged.

### Imagery, icons and content

- The former CSS-built brand glyph was removed.
- BrandMark, onboarding, empty chat and assistant messages use the supplied real crow emblem.
- The unsuitable scientist thumbnail was removed from the primary product identity.
- Lucide icons remain the established functional icon family.
- The supplied diagram and collage are not shipped as small UI art because their baked-in text is unsuitable at application scale.

### States and interactions

- First-run connection test and transition into the workspace were exercised.
- New conversation creation, chat navigation, CrowQuant Memory navigation and the approval/denial flow were exercised.
- The approval modal preserves exact action scope, risk, deny and approve controls.
- Frontend console inspection returned no warnings or errors.
- The automated frontend suite passes all 9 tests and the production frontend build succeeds.

## Resolved findings

- P1: muted text contrast too low — resolved with `#9589a6`.
- P1: primary-button terminal gradient stop too dark — resolved with `#8067ff`.
- P1: scientist scene unreadable as an 88px identity thumbnail — resolved by using the crow emblem at 116px.
- P2: default onboarding overflow — resolved by tightening responsive intro padding; measured shell height and scroll height both equal 760px.
- P2: assistant identity still used the old letter avatar — resolved with the same real emblem used by BrandMark.
- P2: redundant scientist payload and full-resolution decoding in small identity slots — resolved with purpose-sized 96x96, 256x256 and 940x458 WebP derivatives totalling about 150KB.
- P2: tiny functional labels fell below normal-text contrast requirements — resolved by using the verified `#9589a6` muted-text token consistently.
- P1: public QA notes exposed machine-local absolute paths — resolved by retaining only neutral source filenames and roles.
- P2: the minimum-window receipt overstated screenshot pixel evidence — resolved by distinguishing qualitative captures from the recorded 920x640 DOM geometry check.

## Non-blocking follow-up

- P3: native Windows package icons retain the existing violet CrowClaw claw mark. They remain on-brand, while a later installer-asset pass can adopt a formally approved square derivative of the supplied emblem.

## Final result

`passed`

No P0, P1 or P2 design findings remain in the implemented scope.
