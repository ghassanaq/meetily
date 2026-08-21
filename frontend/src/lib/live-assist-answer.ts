const OUTER_EMPHASIS_MARKERS = ['**', '__', '*', '_'] as const

export function displayLiveAssistAnswer(value: string): string {
  const body = value.trimStart()
  const leadingWhitespace = value.slice(0, value.length - body.length)

  for (const marker of OUTER_EMPHASIS_MARKERS) {
    if (!body.startsWith(marker) || body.length <= marker.length) continue

    const firstVisibleCharacter = body.charAt(marker.length)
    if (marker.length === 1 && /\s/.test(firstVisibleCharacter)) continue

    let visible = body.slice(marker.length)
    const withoutTrailingWhitespace = visible.trimEnd()
    const trailingWhitespace = visible.slice(withoutTrailingWhitespace.length)
    if (
      withoutTrailingWhitespace.length > marker.length
      && withoutTrailingWhitespace.endsWith(marker)
    ) {
      visible = withoutTrailingWhitespace.slice(0, -marker.length) + trailingWhitespace
    }

    return leadingWhitespace + visible
  }

  return value
}
