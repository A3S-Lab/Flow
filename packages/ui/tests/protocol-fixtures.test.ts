import { readFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const protocolDir = resolve(
  dirname(fileURLToPath(import.meta.url)),
  "../../../tests/fixtures/protocol",
);

function loadFixture(name: string): unknown {
  return JSON.parse(readFileSync(resolve(protocolDir, name), "utf8"));
}

describe("cross-language FLOW protocol fixtures", () => {
  it("reads the Rust worker capability fixture", () => {
    const caps = loadFixture("worker_capabilities.v1.json") as {
      protocol: string;
      task_types: string[];
      lease_fencing: boolean;
      heartbeats: boolean;
      bounded_drain: boolean;
    };
    expect(caps.protocol).toBe("a3s.flow.worker.v1");
    expect(caps.lease_fencing).toBe(true);
    expect(caps.heartbeats).toBe(true);
    expect(caps.bounded_drain).toBe(true);
    expect(caps.task_types).toContain("drive_run");
    expect(caps.task_types).toContain("resume_scheduled_run");
  });

  it("reads the Rust task and event envelope fixtures", () => {
    const task = loadFixture("flow_task.drive_run.v1.json") as {
      type: string;
      run_id: string;
    };
    expect(task).toEqual({ type: "drive_run", run_id: "cert-run" });

    const envelope = loadFixture("event_envelope.run_created.v1.json") as {
      schema_version: number;
      run_id: string;
      sequence: number;
      event: { type: string; spec: { name: string } };
    };
    expect(envelope.schema_version).toBe(1);
    expect(envelope.run_id).toBe("cert-run");
    expect(envelope.sequence).toBe(1);
    expect(envelope.event.type).toBe("run_created");
    expect(envelope.event.spec.name).toBe("cert.protocol");
  });
});
