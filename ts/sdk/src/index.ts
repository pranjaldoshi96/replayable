/**
 * Replayable TypeScript SDK.
 *
 * v0.0.1 stub. See docs/ARCHITECTURE.md and
 * docs/adr/0001-canonical-trace-schema.md.
 */

export const version = "0.0.1";

export interface AgentTrace {
  traceId: string;
  framework: string;
  metadata: Record<string, unknown>;
}

export function createTrace(traceId: string): AgentTrace {
  return {
    traceId,
    framework: "unknown",
    metadata: {},
  };
}
