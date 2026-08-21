import { describe, expect, it } from 'vitest'
import { authorityIndicator, highlightedAuthoritySentence, type AuthorityWarning } from '../../src/lib/authority-scope-warning'

const warning: AuthorityWarning = {
  code: 'authority_scope_expansion', ruleId: 'operation', ruleLabel: 'Operation boundary',
  sentence: 'I coordinated café work and managed the whole operation.',
  matchedAction: 'managed', matchedContext: null, matchedExcludedObject: 'whole operation',
  excludedStartUtf16: 40, excludedEndUtf16: 55, evidenceRecordIds: ['one'],
}

describe('authority warning presentation', () => {
  it('distinguishes all three honest states', () => {
    expect(authorityIndicator({ status: 'not_configured', evaluatedRuleCount: 0, warnings: [] })).toBe('not_configured')
    expect(authorityIndicator({ status: 'checked_no_match', evaluatedRuleCount: 2, warnings: [] })).toBe('checked')
    expect(authorityIndicator({ status: 'warning', evaluatedRuleCount: 2, warnings: [warning] })).toBe('warning')
  })

  it('uses JavaScript UTF-16 offsets to isolate only the excluded object', () => {
    expect(highlightedAuthoritySentence(warning)).toEqual({
      before: 'I coordinated café work and managed the ', matched: 'whole operation', after: '.',
    })
  })
})
