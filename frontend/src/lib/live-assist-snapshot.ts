export function shouldApplySnapshotResponse(requestId: number, latestAppliedRequestId: number) {
  return Number.isSafeInteger(requestId)
    && Number.isSafeInteger(latestAppliedRequestId)
    && requestId > latestAppliedRequestId
}
