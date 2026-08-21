export type ProfessionalIdentitySummary = {
  id: string
  name: string
  retired_at: string | null
  created_at: string
  updated_at: string
}

export type StoredProfessionalIdentityVersion = {
  identity_id: string
  version_hash: string
  seq: number
  schema_version: number
  created_at: string
}

export type ProfessionalIdentityVersion = {
  schema_version: 1 | 2
  identity: {
    display_name: string
    role_title: string
    organization: string
    professional_summary: string
  }
  records: Array<{
    id: string
    category: 'cv' | 'terms_of_reference' | 'authority' | 'stakeholder' | 'commitment' | 'operating_practice' | 'other'
    title: string
    content: string
    source: { label: string; revision: string }
    updated_at: string
    valid_until: string | null
    conflict_key: string | null
    tags: string[]
  }>
  projects: Array<{
    id: string
    name: string
    role: string
    status: string
    source: { label: string; revision: string }
    updated_at: string
    valid_until: string | null
    tags: string[]
    facts: Array<{
      id: string
      content: string
      source: { label: string; revision: string }
      conflict_key: string | null
      tags: string[]
    }>
  }>
  authority_constraints?: Array<{
    id: string
    label: string
    contexts: string[]
    action_families: Array<'manage' | 'lead' | 'own' | 'oversee' | 'responsible_for' | 'approve' | 'decide'>
    permitted_objects: string[]
    excluded_objects: string[]
    evidence_record_ids: string[]
  }>
}
