/**
 * HTTP client for a host `flow-host-serve` control plane.
 *
 * Endpoints (host-owned; Flow UI only consumes them):
 * - GET  /v1/runs
 * - POST /v1/runs
 * - GET  /v1/runs/{runId}/proposal
 * - POST /v1/runs/{runId}/plan-edits
 * - POST /v1/runs/{runId}/copilot
 * - GET  /v1/runs/{runId}/failure
 * - POST /v1/runs/{runId}/resume-after-failure
 * - GET  /v1/runs/{runId}/progress
 * - GET  /v1/runs/{runId}
 * - GET  /v1/hooks/active
 * - GET  /v1/hooks/{token}
 * - POST /v1/hooks/{token}/resume
 */

import {
  canvasDocumentFromProposalDto,
  type HostCanvasDocument,
  type JsonObject,
  planEditBodyFromCanvas,
} from './host-canvas';

export const APPROVAL_SCHEMA = 'host.flow-approval.v1';

export class HostClientError extends Error {
  readonly status?: number;
  readonly body?: unknown;

  constructor(message: string, status?: number, body?: unknown) {
    super(message);
    this.name = 'HostClientError';
    this.status = status;
    this.body = body;
  }
}

export type FlowHostClientOptions = {
  baseUrl: string;
  fetchImpl?: typeof fetch;
};

export type FlowApprovalRecord = {
  schema_version: string;
  proposal_digest: string;
  tenant_id: string;
  principal_ref: string;
  approved: boolean;
  approved_at_utc: string;
  expires_at_utc: string;
  approval_digest: string;
};

/**
 * One `host.compose-progress.v1` NDJSON frame from
 * `GET /v1/runs/{run_id}/progress` (`adapters/flow-host/src/progress.rs`'s
 * `ComposeProgressEvent`) -- a step-by-step `compose_plan` draft relayed from
 * Code's MCP `notifications/progress` while the `orchestrator.plan` step is
 * still in flight. Non-authoritative: the approved plan/proposal is the only
 * source of truth: this is a fail-soft UI overlay, never a gate on it.
 */
export type HostComposeProgressFrame = {
  schema_version: string;
  flow_run_id: string;
  observed_at_utc: string;
  step_index: number;
  total: number | null;
  agent_id: string;
  status: string;
  progress_token: string;
  progress: number;
  /** `"phase"` on a planning-phase frame (host-derived from the
   * orchestrator's Code event page); absent on compose_plan drafts. */
  kind?: 'phase';
  /** Phase frames only: `model` (thinking), `tool`, or `finished`. */
  phase?: string;
  /** Phase frames only: short orchestrator tool name when `phase` is `tool`. */
  tool?: string;
  /** Phase frames only: model calls so far in this planning dispatch. */
  model_calls?: number;
  /** Phase frames only: characters of model text streamed so far. */
  generated_chars?: number;
  /** Phase frames only: ms since this planning dispatch's first event. */
  elapsed_ms?: number;
};

/**
 * `POST /v1/runs` request body's `TaskEnvelope` half (adapters/flow-host/src/
 * lib.rs's `TaskEnvelope`, `#[serde(flatten)]`d alongside `run_id`).
 * `approval_token` is a caller-minted bearer secret, not host-issued -- the
 * host echoes it back as `approval_token` on the proposal DTO once the run
 * suspends, so the browser can drive Approve through the existing
 * issueApprovalRecord/resumeHook path unchanged.
 */
export type FlowTaskEnvelope = {
  task_text: string;
  permission_ceiling: string[];
  required_capabilities?: string[];
  forbidden_capabilities?: string[];
  protocols?: string[];
  limit?: number;
  approval_token: string;
};

function trimTrailingSlash(url: string): string {
  return url.replace(/\/+$/, '');
}

/**
 * Parse one `host.compose-progress.v1` NDJSON line from
 * `GET /v1/runs/{run_id}/progress`. Malformed/blank lines are skipped, not
 * thrown -- one bad line must never abort the whole stream.
 */
function parseComposeProgressLine(line: string): HostComposeProgressFrame | null {
  const trimmed = line.trim();
  if (!trimmed) return null;
  try {
    const value = JSON.parse(trimmed) as Record<string, unknown>;
    if (
      typeof value.schema_version !== 'string' ||
      typeof value.flow_run_id !== 'string' ||
      typeof value.step_index !== 'number' ||
      typeof value.agent_id !== 'string'
    ) {
      return null;
    }
    return {
      schema_version: value.schema_version,
      flow_run_id: value.flow_run_id,
      observed_at_utc:
        typeof value.observed_at_utc === 'string' ? value.observed_at_utc : '',
      step_index: value.step_index,
      total: typeof value.total === 'number' ? value.total : null,
      agent_id: value.agent_id,
      status: typeof value.status === 'string' ? value.status : '',
      progress_token:
        typeof value.progress_token === 'string' ? value.progress_token : '',
      progress: typeof value.progress === 'number' ? value.progress : 0,
      ...(value.kind === 'phase' ? { kind: 'phase' as const } : {}),
      ...(typeof value.phase === 'string' ? { phase: value.phase } : {}),
      ...(typeof value.tool === 'string' ? { tool: value.tool } : {}),
      ...(typeof value.model_calls === 'number'
        ? { model_calls: value.model_calls }
        : {}),
      ...(typeof value.generated_chars === 'number'
        ? { generated_chars: value.generated_chars }
        : {}),
      ...(typeof value.elapsed_ms === 'number'
        ? { elapsed_ms: value.elapsed_ms }
        : {}),
    };
  } catch {
    return null;
  }
}

/** Canonical JSON matching serde_json BTreeMap key order (ASCII-safe). */
export function canonicalJson(value: unknown): string {
  if (value === null || typeof value !== 'object') {
    return JSON.stringify(value);
  }
  if (Array.isArray(value)) {
    return `[${value.map((item) => canonicalJson(item)).join(',')}]`;
  }
  const record = value as JsonObject;
  const keys = Object.keys(record).sort();
  return `{${keys
    .map((key) => `${JSON.stringify(key)}:${canonicalJson(record[key])}`)
    .join(',')}}`;
}

async function sha256Hex(bytes: Uint8Array): Promise<string> {
  const subtle = globalThis.crypto?.subtle;
  if (!subtle) {
    throw new HostClientError('INVALID_INPUT: Web Crypto subtle is unavailable');
  }
  const copy = new Uint8Array(bytes);
  const digest = await subtle.digest('SHA-256', copy);
  return Array.from(new Uint8Array(digest))
    .map((byte) => byte.toString(16).padStart(2, '0'))
    .join('');
}

/** `sha256:<hex>` over canonical JSON, matching the host digest. */
export async function sha256Digest(value: unknown): Promise<string> {
  const encoded = new TextEncoder().encode(canonicalJson(value));
  return `sha256:${await sha256Hex(encoded)}`;
}

function formatUtc(date: Date): string {
  const iso = date.toISOString();
  return `${iso.slice(0, 19)}Z`;
}

/** Issue `host.flow-approval.v1` with a digest over the body without approval_digest. */
export async function issueApprovalRecord(input: {
  proposalDigest: string;
  tenantId: string;
  principalRef: string;
  approved: boolean;
  approvedAt?: Date;
  ttlMs?: number;
}): Promise<FlowApprovalRecord> {
  const approvedAt = input.approvedAt ?? new Date();
  const ttlMs = input.ttlMs ?? 60 * 60 * 1000;
  const body = {
    schema_version: APPROVAL_SCHEMA,
    proposal_digest: input.proposalDigest,
    tenant_id: input.tenantId,
    principal_ref: input.principalRef,
    approved: input.approved,
    approved_at_utc: formatUtc(approvedAt),
    expires_at_utc: formatUtc(new Date(approvedAt.getTime() + ttlMs)),
  };
  const approval_digest = await sha256Digest(body);
  return { ...body, approval_digest };
}

export const GATE_APPROVAL_SCHEMA = 'host.gate-approval.v1';

/** A reviewer's decision on one mid-plan approval gate of one run. */
export type GateApprovalRecord = {
  schema_version: typeof GATE_APPROVAL_SCHEMA;
  proposal_digest: string;
  run_id: string;
  step_id: string;
  tenant_id: string;
  principal_ref: string;
  approved: boolean;
  comment: string;
  decided_at_utc: string;
  approval_digest: string;
};

/** `GET /v1/runs/{run_id}/gate`: the run's open approval gate. */
export type HostGateRequest = {
  run_id: string;
  step_id: string;
  objective: string;
  proposal_digest: string;
  tenant_id: string;
  /** Upstream outputs the reviewer checks (`host.agent-step-prompt` input shape). */
  inputs: JsonObject[];
};

export function gateRequestFromDto(dto: JsonObject): HostGateRequest | null {
  const metadata = (dto.metadata ?? {}) as JsonObject;
  const stepId = typeof dto.step_id === 'string' ? dto.step_id : '';
  const proposalDigest =
    typeof metadata.proposal_digest === 'string' ? metadata.proposal_digest : '';
  if (!stepId || !proposalDigest) return null;
  return {
    run_id: typeof dto.run_id === 'string' ? dto.run_id : '',
    step_id: stepId,
    objective: typeof metadata.objective === 'string' ? metadata.objective : '',
    proposal_digest: proposalDigest,
    tenant_id: typeof metadata.tenant_id === 'string' ? metadata.tenant_id : '',
    inputs: Array.isArray(metadata.inputs)
      ? (metadata.inputs.filter(
          (input) => input && typeof input === 'object',
        ) as JsonObject[])
      : [],
  };
}

/** Issue `host.gate-approval.v1`: digest over the body without approval_digest
 * (byte-identical to the host's `GateApprovalRecord::issue`). */
export async function issueGateApprovalRecord(input: {
  proposalDigest: string;
  runId: string;
  stepId: string;
  tenantId: string;
  principalRef: string;
  approved: boolean;
  comment?: string;
  decidedAt?: Date;
}): Promise<GateApprovalRecord> {
  const body = {
    schema_version: GATE_APPROVAL_SCHEMA as typeof GATE_APPROVAL_SCHEMA,
    proposal_digest: input.proposalDigest,
    run_id: input.runId,
    step_id: input.stepId,
    tenant_id: input.tenantId,
    principal_ref: input.principalRef,
    approved: input.approved,
    comment: input.comment ?? '',
    decided_at_utc: formatUtc(input.decidedAt ?? new Date()),
  };
  const approval_digest = await sha256Digest(body);
  return { ...body, approval_digest };
}

export class FlowHostClient {
  readonly baseUrl: string;
  private readonly fetchImpl: typeof fetch;

  constructor(options: FlowHostClientOptions) {
    const base = options.baseUrl?.trim();
    if (!base) {
      throw new HostClientError(
        'INVALID_INPUT: host base URL is required for host mode',
      );
    }
    this.baseUrl = trimTrailingSlash(base);
    this.fetchImpl = options.fetchImpl ?? globalThis.fetch.bind(globalThis);
  }

  private url(path: string): string {
    if (!path.startsWith('/')) {
      throw new HostClientError(`INVALID_INPUT: path must be absolute: ${path}`);
    }
    return `${this.baseUrl}${path}`;
  }

  private async request(
    method: string,
    path: string,
    body?: unknown,
  ): Promise<{ status: number; json: unknown }> {
    let response: Response;
    try {
      response = await this.fetchImpl(this.url(path), {
        method,
        headers: {
          Accept: 'application/json',
          ...(body === undefined
            ? {}
            : { 'Content-Type': 'application/json' }),
        },
        body: body === undefined ? undefined : JSON.stringify(body),
      });
    } catch (error) {
      throw new HostClientError(
        `INVALID_INPUT: host request failed: ${
          error instanceof Error ? error.message : String(error)
        }`,
      );
    }
    const text = await response.text();
    let json: unknown = null;
    if (text.trim()) {
      try {
        json = JSON.parse(text) as unknown;
      } catch {
        json = { error: text };
      }
    }
    if (!response.ok) {
      throw new HostClientError(
        `INVALID_INPUT: host returned HTTP ${response.status}`,
        response.status,
        json,
      );
    }
    return { status: response.status, json };
  }

  async getProposal(runId: string): Promise<JsonObject> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    const { json } = await this.request(
      'GET',
      `/v1/runs/${encodeURIComponent(runId)}/proposal`,
    );
    if (!json || typeof json !== 'object') {
      throw new HostClientError('INVALID_INPUT: proposal response is empty');
    }
    return json as JsonObject;
  }

  async openCanvas(runId: string): Promise<HostCanvasDocument> {
    return canvasDocumentFromProposalDto(await this.getProposal(runId));
  }

  async postPlanEdits(
    runId: string,
    body: JsonObject,
  ): Promise<{ status: number; json: JsonObject }> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    try {
      const { status, json } = await this.request(
        'POST',
        `/v1/runs/${encodeURIComponent(runId)}/plan-edits`,
        body,
      );
      return { status, json: (json ?? {}) as JsonObject };
    } catch (error) {
      if (error instanceof HostClientError && error.status !== undefined) {
        return {
          status: error.status,
          json: (error.body ?? {}) as JsonObject,
        };
      }
      throw error;
    }
  }

  async saveCanvas(
    canvas: HostCanvasDocument,
    editRevision: number,
  ): Promise<{ status: number; json: JsonObject }> {
    const body = planEditBodyFromCanvas(canvas, editRevision);
    return this.postPlanEdits(canvas.run_id, body);
  }

  /**
   * `POST /v1/runs/{run_id}/copilot` — never writes to the ledger; the
   * caller applies any `suggested_steps` to the canvas separately (unsaved)
   * and still has to Save/Approve for it to affect execution.
   */
  async postCopilot(
    runId: string,
    body: JsonObject,
  ): Promise<{ status: number; json: JsonObject }> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    try {
      const { status, json } = await this.request(
        'POST',
        `/v1/runs/${encodeURIComponent(runId)}/copilot`,
        body,
      );
      return { status, json: (json ?? {}) as JsonObject };
    } catch (error) {
      if (error instanceof HostClientError && error.status !== undefined) {
        return {
          status: error.status,
          json: (error.body ?? {}) as JsonObject,
        };
      }
      throw error;
    }
  }

  async listActiveHooks(): Promise<JsonObject> {
    const { json } = await this.request('GET', '/v1/hooks/active');
    return (json ?? {}) as JsonObject;
  }

  /**
   * `GET /v1/runs/{run_id}/failure` -- present only while the run has an
   * open recovery hook (a single agent-execution step permanently failed
   * and the run suspended instead of failing outright). A 404 ("no open
   * recovery hook right now") is a normal outcome, not a network failure,
   * so this returns `{status, json}` like `postPlanEdits`/`postCopilot`
   * instead of throwing on non-2xx -- the caller inspects `status` itself.
   */
  async getRecoveryFailure(
    runId: string,
  ): Promise<{ status: number; json: JsonObject }> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    try {
      const { status, json } = await this.request(
        'GET',
        `/v1/runs/${encodeURIComponent(runId)}/failure`,
      );
      return { status, json: (json ?? {}) as JsonObject };
    } catch (error) {
      if (error instanceof HostClientError && error.status !== undefined) {
        return {
          status: error.status,
          json: (error.body ?? {}) as JsonObject,
        };
      }
      throw error;
    }
  }

  /**
   * `GET /v1/runs/{run_id}/progress` -- step-by-step `compose_plan` drafts
   * while `orchestrator.plan` is still in flight, one `host.compose-progress.v1`
   * JSON object per line (`Content-Type: application/x-ndjson`; see
   * `adapters/flow-host/src/progress.rs`/`http.rs`'s `get_progress`).
   * `follow: true` keeps the connection open and yields new frames as they
   * arrive; the default (`false`/omitted) yields the current buffer then
   * ends. A non-2xx response (including 404 -- no host support, or nothing
   * buffered yet) ends the generator without throwing: progress is a
   * fail-soft overlay, never a gate on open/save/approve/recover. `signal`
   * aborts the underlying fetch (e.g. on unmount or once the proposal
   * arrives); an abort also ends the generator without throwing.
   */
  async *getProgressStream(
    runId: string,
    options: { follow?: boolean; signal?: AbortSignal } = {},
  ): AsyncGenerator<HostComposeProgressFrame> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    const path = `/v1/runs/${encodeURIComponent(runId)}/progress${
      options.follow ? '?follow=true' : ''
    }`;
    let response: Response;
    try {
      response = await this.fetchImpl(this.url(path), {
        headers: { Accept: 'application/x-ndjson' },
        signal: options.signal,
      });
    } catch {
      // Network failure or AbortError -- fail-soft, no frames.
      return;
    }
    if (!response.ok || !response.body) {
      return;
    }
    const reader = response.body.getReader();
    const decoder = new TextDecoder();
    let buffer = '';
    try {
      while (true) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        let newlineIndex = buffer.indexOf('\n');
        while (newlineIndex !== -1) {
          const line = buffer.slice(0, newlineIndex);
          buffer = buffer.slice(newlineIndex + 1);
          const frame = parseComposeProgressLine(line);
          if (frame) yield frame;
          newlineIndex = buffer.indexOf('\n');
        }
      }
      const trailing = buffer + decoder.decode();
      const frame = parseComposeProgressLine(trailing);
      if (frame) yield frame;
    } catch {
      // Aborted mid-stream or a transport error -- fail-soft, stop yielding.
    }
  }

  /**
   * `POST /v1/runs/{run_id}/resume-after-failure` -- submit a human-edited
   * recovery decision (`agent_id`+`version`, `objective`, and/or `context`;
   * omitting a field keeps the original value) for the run's currently open
   * recovery hook. On rejection (400 `REJECTED: <codes>: ...`) the hook
   * stays open/resumable, so this never throws on that response -- the
   * caller inspects `status`/`json.error` and may resubmit.
   */
  async postResumeAfterFailure(
    runId: string,
    body: JsonObject,
  ): Promise<{ status: number; json: JsonObject }> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    try {
      const { status, json } = await this.request(
        'POST',
        `/v1/runs/${encodeURIComponent(runId)}/resume-after-failure`,
        body,
      );
      return { status, json: (json ?? {}) as JsonObject };
    } catch (error) {
      if (error instanceof HostClientError && error.status !== undefined) {
        return {
          status: error.status,
          json: (error.body ?? {}) as JsonObject,
        };
      }
      throw error;
    }
  }

  /**
   * `POST /v1/runs` -- start a genuinely new run against the host catalog,
   * unconstrained by any other run's frozen candidate set. `runId` is
   * caller-chosen (the host does not
   * generate one); reusing an existing id with a different envelope is
   * rejected by the engine as a conflict, so this always needs a fresh id
   * for a fresh task.
   */
  async startRun(
    runId: string,
    envelope: FlowTaskEnvelope,
  ): Promise<{ status: number; json: JsonObject }> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    try {
      const { status, json } = await this.request('POST', '/v1/runs', {
        run_id: runId,
        ...envelope,
      });
      return { status, json: (json ?? {}) as JsonObject };
    } catch (error) {
      if (error instanceof HostClientError && error.status !== undefined) {
        return {
          status: error.status,
          json: (error.body ?? {}) as JsonObject,
        };
      }
      throw error;
    }
  }

  /**
   * `GET /v1/runs/{run_id}` -- the Flow `WorkflowRunSnapshot` (run `status`
   * plus per-step `steps[<flow step id>].status`). The only host endpoint
   * that reflects execution progress *after* approval: the proposal DTO's
   * `flow_status` is re-read only on explicit refetch, and the progress
   * NDJSON stream covers `compose_plan` drafting only.
   */
  async getRun(runId: string): Promise<JsonObject> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    const { json } = await this.request(
      'GET',
      `/v1/runs/${encodeURIComponent(runId)}`,
    );
    return (json ?? {}) as JsonObject;
  }

  /** `GET /v1/runs` -- every run this host's store knows about, for the run-history picker. */
  async listRuns(): Promise<JsonObject> {
    const { json } = await this.request('GET', '/v1/runs');
    return (json ?? {}) as JsonObject;
  }

  async getHook(token: string): Promise<JsonObject> {
    if (!token.trim()) {
      throw new HostClientError('INVALID_INPUT: hook token is required');
    }
    const { json } = await this.request(
      'GET',
      `/v1/hooks/${encodeURIComponent(token)}`,
    );
    return (json ?? {}) as JsonObject;
  }

  /**
   * `GET /v1/runs/{run_id}/gate` -- present only while the run waits on a
   * mid-plan approval gate. A 404 ("no open gate") is a normal outcome, so
   * this returns `{status, json}` like `getRecoveryFailure`.
   */
  async getOpenGate(runId: string): Promise<{ status: number; json: JsonObject }> {
    if (!runId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId is required');
    }
    try {
      const { status, json } = await this.request(
        'GET',
        `/v1/runs/${encodeURIComponent(runId)}/gate`,
      );
      return { status, json: (json ?? {}) as JsonObject };
    } catch (error) {
      if (error instanceof HostClientError && error.status !== undefined) {
        return { status: error.status, json: (error.body ?? {}) as JsonObject };
      }
      throw error;
    }
  }

  /** `POST /v1/runs/{run_id}/gate/{step_id}` -- decide the open gate; the
   * host drives the run forward inside this request. */
  async decideGate(
    runId: string,
    stepId: string,
    record: GateApprovalRecord,
  ): Promise<JsonObject> {
    if (!runId.trim() || !stepId.trim()) {
      throw new HostClientError('INVALID_INPUT: runId and stepId are required');
    }
    const { json } = await this.request(
      'POST',
      `/v1/runs/${encodeURIComponent(runId)}/gate/${encodeURIComponent(stepId)}`,
      record,
    );
    return (json ?? {}) as JsonObject;
  }

  async resumeHook(
    token: string,
    approval: FlowApprovalRecord,
  ): Promise<JsonObject> {
    if (!token.trim()) {
      throw new HostClientError('INVALID_INPUT: hook token is required');
    }
    const { json } = await this.request(
      'POST',
      `/v1/hooks/${encodeURIComponent(token)}/resume`,
      approval,
    );
    return (json ?? {}) as JsonObject;
  }
}

export function parseHostModeSearch(search: string): {
  host: string;
  runId: string;
  tenantId: string;
  principalRef: string;
} | null {
  const params = new URLSearchParams(
    search.startsWith('?') ? search.slice(1) : search,
  );
  const host = (params.get('host') ?? '').trim();
  const runId = (params.get('runId') ?? params.get('run_id') ?? '').trim();
  if (!host && !runId) return null;
  if (!host) {
    // A bare `runId` with no `host` can never be resolved -- still an error.
    // `host` alone is valid: the run-history picker state (no run selected
    // yet), so `runId` may be `''` here.
    throw new HostClientError(
      'INVALID_INPUT: host mode requires a host query param',
    );
  }
  return {
    host,
    runId,
    tenantId: (params.get('tenant') ?? 'tenant-local-validation').trim(),
    principalRef: (params.get('principal') ?? 'reviewer@local').trim(),
  };
}
