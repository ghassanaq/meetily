import { describe, expect, test } from "vitest";
import {
  getDownloadTotalMb,
  getSummaryModelSizeLabel,
  getSummaryModelSizeMb,
  resolveOnboardingSummaryModelStatus,
} from "../../src/lib/onboarding-summary-model";

describe("onboarding summary model", () => {
  test("uses the selected model's readiness", () => {
    expect(
      resolveOnboardingSummaryModelStatus({
        selectedModel: "qwen3.5:4b",
        recommendedModel: "qwen3.5:4b",
        selectedModelReady: false,
      }),
    ).toEqual({
      selectedSummaryModel: "qwen3.5:4b",
      summaryModelDownloaded: false,
    });

    expect(
      resolveOnboardingSummaryModelStatus({
        selectedModel: "gemma3:1b",
        recommendedModel: "qwen3.5:4b",
        selectedModelReady: true,
      }),
    ).toEqual({
      selectedSummaryModel: "gemma3:1b",
      summaryModelDownloaded: true,
    });

    expect(
      resolveOnboardingSummaryModelStatus({
        selectedModel: "",
        recommendedModel: "qwen3.5:2b",
        selectedModelReady: true,
      }),
    ).toEqual({
      selectedSummaryModel: "qwen3.5:2b",
      summaryModelDownloaded: true,
    });
  });

  test("reports model sizes", () => {
    expect(getSummaryModelSizeMb("qwen3.5:2b")).toBe(1221);
    expect(getSummaryModelSizeMb("qwen3.5:4b")).toBe(2614);
    expect(getSummaryModelSizeMb("gemma3:1b")).toBe(1019);
    expect(getSummaryModelSizeMb("unknown:model")).toBe(0);

    expect(getSummaryModelSizeLabel("qwen3.5:2b")).toBe("~1.2 GiB");
    expect(getSummaryModelSizeLabel("qwen3.5:4b")).toBe("~2.6 GiB");
    expect(getSummaryModelSizeLabel("unknown:model")).toBe("");
  });

  test("prefers an explicit download total", () => {
    expect(getDownloadTotalMb(0, "qwen3.5:4b")).toBe(2614);
    expect(getDownloadTotalMb(undefined, "qwen3.5:2b")).toBe(1221);
    expect(getDownloadTotalMb(512, "qwen3.5:4b")).toBe(512);
  });
});
