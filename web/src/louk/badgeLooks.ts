// louk-fork: the fork's badge looks, offered as extra "Badge background"
// options. Values are the API tokens (BadgeBackground in api/src/services/db.rs);
// the drawing and design notes are in api/src/louk/badge_looks.rs and
// docs/louk/badge-styles.md.

export interface BadgeLook {
  value: string
  label: string
  description: string
}

export const LOUK_BADGE_LOOKS: readonly BadgeLook[] = [
  {
    value: 's',
    label: 'Score colour',
    description: 'Dark plate with a bar that fills with the score: teal for good, amber for middling, red for weak.',
  },
  {
    value: 'm',
    label: 'Mono',
    description: 'Quiet dark plate with white numbers and white source glyphs, no colour.',
  },
  {
    value: 'a',
    label: 'One score (average)',
    description: 'One figure, the average of the shown ratings on a 0–10 scale, with a ring that fills with it.',
  },
]

/** The one-line description for a fork look, or undefined for upstream values. */
export function badgeLookDescription(value: string): string | undefined {
  return LOUK_BADGE_LOOKS.find((look) => look.value === value)?.description
}
