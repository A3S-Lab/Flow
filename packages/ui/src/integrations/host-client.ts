/**
 * HTTP client for a host `flow-host-serve` control plane.
 *
 * Endpoints (host-owned; Flow UI only consumes them):
 * - GET  /v1/runs
 * - POST /v1/runs
 * - GET  /v1/runs/{runId}/proposal
 * - POST /v1/runs/{runId}/plan-edits
 * - POST /v1/runs/{runId}/copilot
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
