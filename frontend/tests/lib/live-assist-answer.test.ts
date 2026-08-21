import { describe, expect, test } from 'vitest'
import { displayLiveAssistAnswer } from '../../src/lib/live-assist-answer'

describe('Live Assist answer display', () => {
  test('hides an opening emphasis marker while the answer is streaming', () => {
    expect(displayLiveAssistAnswer('**I lead')).toBe('I lead')
    expect(displayLiveAssistAnswer('__I lead')).toBe('I lead')
  })

  test('removes matching outer emphasis after completion', () => {
    expect(displayLiveAssistAnswer('**I lead**')).toBe('I lead')
    expect(displayLiveAssistAnswer('_I lead_')).toBe('I lead')
    expect(displayLiveAssistAnswer('  **I lead**  ')).toBe('  I lead  ')
  })

  test('does not alter inline Markdown or a bullet-like prefix', () => {
    expect(displayLiveAssistAnswer('I use **evidence**')).toBe('I use **evidence**')
    expect(displayLiveAssistAnswer('* bullet')).toBe('* bullet')
  })
})
