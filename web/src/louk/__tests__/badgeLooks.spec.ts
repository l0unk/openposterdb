import { describe, it, expect } from 'vitest'
import { LOUK_BADGE_LOOKS, badgeLookDescription } from '@/louk/badgeLooks'
import formSource from '@/components/RenderSettingsForm.vue?raw'

describe('LOUK_BADGE_LOOKS', () => {
  it('lists the three fork looks with the API tokens', () => {
    expect(LOUK_BADGE_LOOKS.map((l) => l.value)).toEqual(['s', 'm', 'a'])
  })

  it('never reuses an upstream background token', () => {
    for (const look of LOUK_BADGE_LOOKS) {
      expect(['d', 'k', 't', 'n']).not.toContain(look.value)
    }
  })

  it('gives every look a name and a one-line description', () => {
    for (const look of LOUK_BADGE_LOOKS) {
      expect(look.label.length).toBeGreaterThan(0)
      expect(look.description.length).toBeGreaterThan(0)
      expect(look.description).not.toContain('\n')
    }
  })

  it('describes fork looks only', () => {
    expect(badgeLookDescription('s')).toBe(LOUK_BADGE_LOOKS[0]!.description)
    expect(badgeLookDescription('d')).toBeUndefined()
    expect(badgeLookDescription('k')).toBeUndefined()
  })
})

describe('RenderSettingsForm offers fork looks', () => {
  it('adds the fork looks to all four badge background selects', () => {
    const src: string = formSource
    const loops = src.match(/v-for="look in LOUK_BADGE_LOOKS"/g) ?? []
    expect(loops.length).toBe(4)
    // The upstream default stays the first option.
    expect(src).toContain('<SelectItem value="d">Default</SelectItem>')
  })
})
