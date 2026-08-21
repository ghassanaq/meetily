export type AuthorityWarning = {
  code: 'authority_scope_expansion'
  ruleId: string
  ruleLabel: string
  sentence: string
  matchedAction: string
  matchedContext: string | null
  matchedExcludedObject: string
  excludedStartUtf16: number
  excludedEndUtf16: number
  evidenceRecordIds: string[]
}

export type AuthorityCheck = {
  status: 'not_configured' | 'checked_no_match' | 'warning'
  evaluatedRuleCount: number
  warnings: AuthorityWarning[]
}

export function authorityIndicator(check: AuthorityCheck) {
  if (check.status === 'warning') return 'warning' as const
  if (check.status === 'checked_no_match') return 'checked' as const
  return 'not_configured' as const
}

export function highlightedAuthoritySentence(warning: AuthorityWarning) {
  const { sentence, excludedStartUtf16: start, excludedEndUtf16: end } = warning
  return {
    before: sentence.slice(0, start),
    matched: sentence.slice(start, end),
    after: sentence.slice(end),
  }
}
