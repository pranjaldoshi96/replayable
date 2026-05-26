import { describe, expect, it } from "vitest";
import { createTrace, version } from "../src/index.js";

describe("@replayable/sdk", () => {
  it("exposes a version string", () => {
    expect(version).toMatch(/^0\./);
  });

  it("creates an AgentTrace with defaults", () => {
    const t = createTrace("abc-123");
    expect(t.traceId).toBe("abc-123");
    expect(t.framework).toBe("unknown");
    expect(t.metadata).toEqual({});
  });
});
