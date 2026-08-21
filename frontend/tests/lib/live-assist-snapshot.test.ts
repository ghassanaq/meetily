import { describe, expect, test } from 'vitest'
import { shouldApplySnapshotResponse } from '../../src/lib/live-assist-snapshot'

describe('Live Assist snapshot ordering', () => {
  test('accepts newer responses and rejects stale overlapping responses', () => {
    expect(shouldApplySnapshotResponse(1, 0)).toBe(true)
    expect(shouldApplySnapshotResponse(3, 1)).toBe(true)
    expect(shouldApplySnapshotResponse(2, 3)).toBe(false)
    expect(shouldApplySnapshotResponse(3, 3)).toBe(false)
  })

  test('rejects malformed request counters', () => {
    expect(shouldApplySnapshotResponse(Number.NaN, 0)).toBe(false)
    expect(shouldApplySnapshotResponse(1.5, 0)).toBe(false)
  })
})
