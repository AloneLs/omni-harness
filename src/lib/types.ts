export type TaskStateEnum =
  | "CREATED"
  | "CONTRACT_BUILDING"
  | "PLANNING"
  | "EXECUTING"
  | "WAITING_TOOL"
  | "REVIEWING"
  | "VERIFYING"
  | "COMPLETED"
  | "REPAIRING"
  | "BLOCKED"
  | "FAILED";

export interface Provider {
  id: string;
  name: string;
  providerType: string;
  baseUrl: string;
  apiKeyName: string;
  isActive: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface Model {
  id: string;
  providerId: string;
  modelName: string;
  displayName: string;
  contextWindow: number;
  maxOutputTokens: number;
  supportsReasoning: boolean;
  supportsVision: boolean;
  supportsTools: boolean;
  inputPricePerM: number;
  outputPricePerM: number;
  isActive: boolean;
  createdAt: string;
}

export interface Project {
  id: string;
  name: string;
  rootPath: string;
  createdAt: string;
  updatedAt: string;
}

export interface Chat {
  id: string;
  projectId?: string;
  title: string;
  modelId: string;
  systemPrompt: string;
  isPinned: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface Message {
  id: string;
  chatId: string;
  role: "system" | "user" | "assistant" | "tool";
  content: string;
  reasoningContent?: string;
  toolCalls?: string;
  toolCallId?: string;
  sequenceNum: number;
  createdAt: string;
}

export interface FileSnapshot {
  id: string;
  chatId: string;
  messageId?: string;
  filePath: string;
  beforeBlob?: number[];
  afterBlob?: number[];
  existedBefore: boolean;
  addedLines: number;
  removedLines: number;
  createdAt: string;
}

export interface MemoryEntry {
  id: string;
  category: string;
  key: string;
  content: string;
  tokenCount: number;
  createdAt: string;
  updatedAt: string;
}

export interface McpServer {
  id: string;
  name: string;
  transportType: "stdio" | "sse" | "streamable_http";
  command?: string;
  args?: string;
  env?: string;
  url?: string;
  isEnabled: boolean;
  createdAt: string;
  updatedAt: string;
}

export interface ScheduledJob {
  id: string;
  name: string;
  cronExpression: string;
  prompt: string;
  modelId: string;
  chatId?: string;
  isActive: boolean;
  lastRunAt?: string;
  nextRunAt?: string;
  createdAt: string;
  updatedAt: string;
}

export interface ModelUsageStat {
  id: string;
  providerId: string;
  modelId: string;
  chatId: string;
  promptTokens: number;
  completionTokens: number;
  totalTokens: number;
  estimatedCost: number;
  createdAt: string;
}

export interface BootstrapConfig {
  dataDir?: string;
  workspaceDir?: string;
  firstRunCompleted: boolean;
  defaultModelId?: string;
  telemetryEnabled: boolean;
}

export interface AcceptanceCriterion {
  id: string;
  description: string;
  status: "PENDING" | "IN_PROGRESS" | "PASS" | "FAIL" | "BLOCKED" | "NOT_VERIFIED";
  evidence?: string;
}

export interface TaskContract {
  goal: string;
  criteria: AcceptanceCriterion[];
  definitionOfDone: string[];
}

export interface StructuredVerdict {
  verdict: "PASS" | "FAIL" | "NEEDS_REPAIR";
  criteriaUpdates: Array<{ id: string; status: string; evidence?: string }>;
  blockingIssues: string[];
  repairInstructions: string[];
}

export interface SkillMeta {
  name: string;
  title: string;
  description: string;
  command: string;
  version: string;
  author: string;
  tags: string[];
  keywords: string[];
  autoStart: boolean;
  enabled: boolean;
  trusted: boolean;
}

export interface ToolExecutionRecord {
  id: string;
  toolName: string;
  argsPreview: string;
  status: "running" | "success" | "error";
  output?: string;
  durationMs?: number;
  fileDiff?: { path: string; added: number; removed: number };
}

export interface TelegramConfig {
  botToken: string;
  pairingCode: string;
  allowedChatIds: number[];
  isRunning: boolean;
}

export interface SpeechConfig {
  modelName: string;
  downloadProgress: number;
  inputDevice: string;
  isTranscribing: boolean;
}

export type StreamEvent =
  | { type: "delta"; content: string }
  | { type: "reasoning_delta"; content: string }
  | { type: "tool_call_start"; id: string; name: string }
  | { type: "tool_call_delta"; id: string; argumentsDelta: string }
  | { type: "tool_call_complete"; id: string; name: string; arguments: string }
  | { type: "tool_result"; id: string; output: string; isError: boolean }
  | { type: "retroactive_reasoning_move"; reasoningContent: string }
  | { type: "retry_notice"; attempt: number; reason: string }
  | { type: "status_change"; state: TaskStateEnum }
  | { type: "contract_updated"; contract: TaskContract }
  | { type: "verdict"; verdict: StructuredVerdict }
  | { type: "done"; usage?: ModelUsageStat }
  | { type: "error"; message: string };
