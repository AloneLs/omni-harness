import { invoke, Channel } from '@tauri-apps/api/core';
import type { ToolExecutionRecord } from '../toolMapper';
import { formatToolExecution, isInternalArtifactTool } from '../toolMapper';
import { settingsStore, resolveModelForReasoning, resolveValidModelForProvider, findModelGroup, isImageGenerationModel, type ReasoningLevel } from './settings.svelte';
import { workspaceStore } from './workspace.svelte';

export interface ChatAttachment {
  id: string;
  name: string;
  path: string;
  size?: number;
  kind: 'image' | 'video' | 'audio' | 'file';
  previewUrl?: string;
}

export interface TodoBoardItem {
  id: string;
  text: string;
  status: 'pending' | 'in_progress' | 'completed' | 'failed' | 'blocked';
}

export type TimelineStep =
  | { kind: 'thought'; id: string; text: string; startedAt: number; finishedAt?: number }
  | { kind: 'tool'; record: ToolExecutionRecord }
  | { kind: 'message'; id: string; text: string }
  | { kind: 'compact'; id: string; beforeTokens: number; afterTokens: number; summary: string };

export interface ChatMessage {
  id: string;
  chatId: string;
  role: 'user' | 'assistant' | 'system' | 'tool';
  content: string;
  modelId?: string;
  attachments?: ChatAttachment[];
  reasoningContent?: string;
  toolCalls?: ToolExecutionRecord[];
  steps?: TimelineStep[];
  todos?: TodoBoardItem[];
  statusText?: string;
  retryReason?: string;
  startedAt?: number;
  finishedAt?: number;
  imageGenerating?: boolean;
  error?: string;
  createdAt: number;
  streaming?: boolean;
  paused?: boolean;
  snapshotId?: string;
  modifiedFiles?: string[];
  tokenUsage?: {
    promptTokens: number;
    completionTokens: number;
    reasoningTokens?: number;
  };
}

export interface ChatSession {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  pinned: boolean;
  archived?: boolean;
  isDraft?: boolean;
  isThemeStudio?: boolean;
  themeId?: string;
  projectPath?: string;
  providerId?: string;
  modelId?: string;
  hasError?: boolean;
  isPaused?: boolean;
  hasUnreadCompletion?: boolean;
  messageCount: number;
  preview: string;
}

export interface PendingQueueItem {
  id: string;
  text: string;
  createdAt: number;
  priority?: 'normal' | 'intervention';
  source?: string;
  attachments?: ChatAttachment[];
}

export interface GoalStatusInfo {
  status: 'active' | 'achieved' | 'paused' | 'failed' | 'idle' | 'cleared';
  condition: string;
  turn: number;
  maxTurns: number;
  reason?: string | null;
  elapsedSecs: number;
}

export interface ContextTelemetryData {
  usedTokens: number;
  contextWindow: number;
  cachedTokens: number;
  cacheHitRate: number;
}

export interface FilePreviewRequest {
  path: string;
  addedTexts?: string[];
  removedTexts?: string[];
  fallbackContent?: string;
}

export interface TurnFileDiff {
  path: string;
  beforeText: string;
  afterText: string;
  addedLines: number;
  removedLines: number;
  existedBefore: boolean;
}

export interface InlineReviewComment {
  id: string;
  path: string;
  lineNumber: number;
  lineType: 'add' | 'del' | 'ctx';
  lineText: string;
  comment: string;
}

class ChatStore {
  sessions = $state<ChatSession[]>([]);
  draftSession = $state<ChatSession | null>(null);
  activeChatId = $state<string | null>(null);
  imagePreview = $state<{ src: string; alt?: string } | null>(null);
  messagesByChat = $state<Record<string, ChatMessage[]>>({});
  streamingChats = $state<Record<string, boolean>>({});
  pendingQueueByChat = $state<Record<string, PendingQueueItem[]>>({});
  draftTextByChat = $state<Record<string, string>>({});
  draftAttachmentsByChat = $state<Record<string, ChatAttachment[]>>({});
  todosByChat = $state<Record<string, TodoBoardItem[]>>({});
  contextTelemetryByChat = $state<Record<string, ContextTelemetryData>>({});
  goalByChat = $state<Record<string, GoalStatusInfo | null>>({});
  pendingPlanByChat = $state<Record<string, { summary: string } | null>>({});
  reasoningElapsedByChat = $state<Record<string, number>>({});
  streamTokenCountByChat = $state<Record<string, number>>({});

  filterSearchQuery = $state<string>('');
  showArchived = $state<boolean>(false);
  agentMode = $state<'build' | 'plan'>('build');
  filePreview = $state<FilePreviewRequest | null>(null);
  diffReview = $state<{
    messageId: string;
    title: string;
    focusPath?: string;
    files: TurnFileDiff[];
    loading: boolean;
  } | null>(null);
  inlineReviewComments = $state<InlineReviewComment[]>([]);
  sideQuestion = $state<{
    chatId: string;
    question: string;
    answer: string;
    streaming: boolean;
    error: string | null;
    visible: boolean;
  } | null>(null);

  private todoHideTimers: Record<string, ReturnType<typeof setTimeout>> = {};
  private reasoningTimers: Record<string, number> = {};
  private resolveByChat: Record<string, (() => void) | null> = {};
  private streamSaveTimers: Record<string, number> = {};
  private lastDbStreamSyncAt: Record<string, number> = {};
  private pendingTextByChat: Record<string, string> = {};
  private turnSnapshotByChat: Record<string, { contentLen: number; reasoningLen: number; stepsLen: number; toolsLen: number }> = {};
  private pendingReasoningByChat: Record<string, string> = {};
  private smoothRafByChat: Record<string, number | null> = {};
  private activeSmoothMsgByChat: Record<string, string | null> = {};
  private activeStreamMsgIdByChat: Record<string, string | null> = {};
  private canceledMsgIds = new Set<string>();
  private draftSaveTimer: ReturnType<typeof setTimeout> | null = null;
  private unlisteners: Array<() => void> = [];
  private unloadHandler: (() => void) | null = null;
  private disposed = false;
  private pullInFlight = false;
  private pendingPullTargets = new Set<string>();
  private pendingPullAll = false;
  private saveChainByChat: Record<string, Promise<void>> = {};
  private sideQuestionReqId = 0;

  messages = $derived.by(() => {
    return this.activeChatId ? this.messagesByChat[this.activeChatId] || [] : [];
  });

  isStreaming = $derived.by(() => {
    return Boolean(this.activeChatId && this.streamingChats[this.activeChatId]);
  });

  pendingQueue = $derived.by(() => {
    return this.activeChatId ? this.pendingQueueByChat[this.activeChatId] || [] : [];
  });

  pendingPlan = $derived.by(() => {
    return this.activeChatId ? this.pendingPlanByChat[this.activeChatId] || null : null;
  });

  activeGoal = $derived.by(() => {
    return this.activeChatId ? this.goalByChat[this.activeChatId] || null : null;
  });

  activeTodos = $derived.by(() => {
    return this.activeChatId ? this.todosByChat[this.activeChatId] || [] : [];
  });

  currentStreamTokenCount = $derived.by(() => {
    return this.activeChatId ? this.streamTokenCountByChat[this.activeChatId] || 0 : 0;
  });

  reasoningElapsedMs = $derived.by(() => {
    return this.activeChatId ? this.reasoningElapsedByChat[this.activeChatId] || 0 : 0;
  });

  contextTelemetry = $derived.by<ContextTelemetryData>(() => {
    if (this.activeChatId && this.contextTelemetryByChat[this.activeChatId]) {
      return this.contextTelemetryByChat[this.activeChatId];
    }
    const sess = this.activeSession;
    const provId = sess?.providerId || settingsStore.data.activeProvider;
    const modelId = sess?.modelId || settingsStore.data.activeModel;
    const prov = settingsStore.data.providers[provId] || settingsStore.currentProvider;
    const caps = prov?.modelCapabilities?.[modelId] || settingsStore.currentModelCapabilities;
    return {
      usedTokens: 0,
      contextWindow: caps.contextWindow || 128000,
      cachedTokens: 0,
      cacheHitRate: 0
    };
  });

  hasPausedTurn = $derived.by(() => {
    if (this.isStreaming) return false;
    const list = this.messages;
    for (let i = list.length - 1; i >= 0; i--) {
      if (list[i].role === 'system') continue;
      if (list[i].role === 'assistant') {
        return Boolean(list[i].paused);
      }
      if (list[i].role === 'user') {
        return true;
      }
      return false;
    }
    return false;
  });

  activeSession = $derived.by(() => {
    return this.sessions.find((s) => s.id === this.activeChatId) || null;
  });

  pinnedSessions = $derived.by(() => {
    const q = this.filterSearchQuery.toLowerCase().trim();
    return this.sessions.filter(
      (s) =>
        !s.isThemeStudio &&
        !s.isDraft &&
        !s.archived &&
        s.pinned &&
        !s.projectPath &&
        Boolean(s.title) &&
        (!q || s.title.toLowerCase().includes(q) || s.preview.toLowerCase().includes(q))
    );
  });

  recentSessions = $derived.by(() => {
    const q = this.filterSearchQuery.toLowerCase().trim();
    return this.sessions.filter((s) => {
      if (s.isThemeStudio || s.isDraft || !s.title) return false;
      if (s.title === 'New task' && (!this.messagesByChat[s.id] || this.messagesByChat[s.id].length === 0)) return false;
      const matchArchive = this.showArchived ? Boolean(s.archived) : !s.archived && !(s.pinned && !s.projectPath);
      return matchArchive && (!q || s.title.toLowerCase().includes(q) || s.preview.toLowerCase().includes(q));
    });
  });

  archivedCount = $derived.by(() => {
    return this.sessions.filter((s) => !s.isDraft && !s.isThemeStudio && Boolean(s.archived)).length;
  });

  private baseTimelineStats = $derived.by(() => {
    let totalTools = 0;
    let activeTools = 0;
    let promptTokens = 0;
    let completionTokens = 0;

    for (const msg of this.messages) {
      if (msg.toolCalls) {
        totalTools += msg.toolCalls.length;
        for (const tc of msg.toolCalls) {
          if (tc.status === 'running' || tc.status === 'pending') {
            activeTools++;
          }
        }
      }
      if (msg.tokenUsage) {
        promptTokens += msg.tokenUsage.promptTokens || 0;
        completionTokens += msg.tokenUsage.completionTokens || 0;
      }
    }

    return {
      totalToolsExecuted: totalTools,
      activeToolsCount: activeTools,
      promptTokens,
      completionTokens
    };
  });

  timelineStats = $derived.by(() => {
    const base = this.baseTimelineStats;
    const streamTok = this.currentStreamTokenCount;
    return {
      totalToolsExecuted: base.totalToolsExecuted,
      activeToolsCount: base.activeToolsCount,
      reasoningElapsedMs: this.reasoningElapsedMs,
      totalTokens: base.promptTokens + base.completionTokens + streamTok,
      promptTokens: base.promptTokens,
      completionTokens: base.completionTokens + streamTok
    };
  });

  constructor() {
    if (typeof localStorage !== 'undefined') {
      try {
        const rawDrafts = localStorage.getItem('omni_chat_drafts');
        if (rawDrafts) {
          const parsedDrafts = JSON.parse(rawDrafts);
          if (parsedDrafts && typeof parsedDrafts === 'object') {
            if (parsedDrafts.texts && typeof parsedDrafts.texts === 'object') {
              this.draftTextByChat = parsedDrafts.texts;
            }
            if (parsedDrafts.attachments && typeof parsedDrafts.attachments === 'object') {
              this.draftAttachmentsByChat = parsedDrafts.attachments;
            }
          }
        }
      } catch {}
      try {
        const savedActive = localStorage.getItem('omni_active_chat_id');
        if (savedActive) {
          this.activeChatId = savedActive;
        }
      } catch {}
    }
    if (typeof window !== 'undefined') {
      this.unloadHandler = () => {
        this.flushAllBeforeClose();
      };
      window.addEventListener('beforeunload', this.unloadHandler);
      window.addEventListener('pagehide', this.unloadHandler);
    }
    this.init();
  }

  public destroy(): void {
    this.disposed = true;
    this.flushAllBeforeClose();
    if (this.draftSaveTimer) {
      clearTimeout(this.draftSaveTimer);
      this.draftSaveTimer = null;
    }
    if (this.unloadHandler && typeof window !== 'undefined') {
      window.removeEventListener('beforeunload', this.unloadHandler);
      window.removeEventListener('pagehide', this.unloadHandler);
      this.unloadHandler = null;
    }
    for (const cid of Object.keys(this.todoHideTimers)) {
      clearTimeout(this.todoHideTimers[cid]);
    }
    for (const cid of Object.keys(this.reasoningTimers)) {
      clearInterval(this.reasoningTimers[cid]);
    }
    for (const cid of Object.keys(this.streamSaveTimers)) {
      clearTimeout(this.streamSaveTimers[cid]);
    }
    for (const cid of Object.keys(this.smoothRafByChat)) {
      const raf = this.smoothRafByChat[cid];
      if (raf) cancelAnimationFrame(raf);
    }
    this.todoHideTimers = {};
    this.reasoningTimers = {};
    this.streamSaveTimers = {};
    this.smoothRafByChat = {};
    for (const fn of this.unlisteners) {
      try {
        fn();
      } catch {}
    }
    this.unlisteners = [];
  }

  private cleanupChatResources(id: string): void {
    if (this.todoHideTimers[id]) {
      clearTimeout(this.todoHideTimers[id]);
      delete this.todoHideTimers[id];
    }
    if (this.reasoningTimers[id]) {
      clearInterval(this.reasoningTimers[id]);
      delete this.reasoningTimers[id];
    }
    if (this.streamSaveTimers[id]) {
      clearTimeout(this.streamSaveTimers[id]);
      delete this.streamSaveTimers[id];
    }
    const raf = this.smoothRafByChat[id];
    if (raf) {
      cancelAnimationFrame(raf);
    }
    delete this.smoothRafByChat[id];
    delete this.pendingTextByChat[id];
    delete this.pendingReasoningByChat[id];
    delete this.activeSmoothMsgByChat[id];
    delete this.activeStreamMsgIdByChat[id];
    delete this.turnSnapshotByChat[id];
    delete this.resolveByChat[id];
    delete this.todosByChat[id];
    delete this.contextTelemetryByChat[id];
    delete this.goalByChat[id];
    delete this.pendingPlanByChat[id];
    delete this.reasoningElapsedByChat[id];
    delete this.streamTokenCountByChat[id];
    delete this.streamingChats[id];
    delete this.pendingQueueByChat[id];
    delete this.draftTextByChat[id];
    delete this.draftAttachmentsByChat[id];
    if (typeof localStorage !== 'undefined') {
      try {
        localStorage.removeItem(`omni_chat_msgs_${id}`);
      } catch {}
    }
  }

  public flushAllBeforeClose(): void {
    this.flushDraftsNow();
    const chatIds = new Set<string>();
    if (this.activeChatId) chatIds.add(this.activeChatId);
    for (const cid of Object.keys(this.streamingChats)) {
      if (this.streamingChats[cid]) chatIds.add(cid);
    }
    for (const cid of Object.keys(this.streamSaveTimers)) {
      clearTimeout(this.streamSaveTimers[cid]);
      delete this.streamSaveTimers[cid];
      chatIds.add(cid);
    }
    for (const cid of chatIds) {
      this.flushSmoothBuffers(cid);
      this.saveLocalOnly(cid);
      const sess = this.sessions.find((s) => s.id === cid);
      if (sess && !sess.isDraft && !sess.isThemeStudio && this.messagesByChat[cid]) {
        const snapshotMsgs = this.buildRichSyncPayload(this.messagesByChat[cid]);
        invoke('sync_chat_context', {
          chatId: cid,
          title: sess.title || cid,
          messagesJson: JSON.stringify(snapshotMsgs),
          projectPath: sess.projectPath || null,
          modelId: sess.modelId || settingsStore.data.activeModel || null,
          isPinned: Boolean(sess.pinned)
        }).catch(() => {});
      }
    }
  }

  private buildRichSyncPayload(msgs: ChatMessage[]): any[] {
    return this.compactMessagesForStorage(msgs, true).map((m) => ({
      id: m.id,
      chatId: m.chatId,
      role: m.role,
      content: m.content,
      modelId: m.modelId,
      attachments: m.attachments,
      reasoningContent: m.reasoningContent,
      toolCalls: m.toolCalls,
      steps: m.steps,
      todos: m.todos,
      statusText: m.statusText,
      startedAt: m.startedAt,
      finishedAt: m.finishedAt,
      error: m.error,
      streaming: Boolean(m.streaming),
      paused: Boolean(m.paused),
      tokenUsage: m.tokenUsage,
      createdAt: m.createdAt
    }));
  }

  private normalizeLoadedMessages(chatId: string, parsedMsgs: ChatMessage[]): { msgs: ChatMessage[]; changed: boolean } {
    let changed = false;
    const isCurrentlyStreaming = Boolean(this.streamingChats[chatId]);
    for (const m of parsedMsgs) {
      if (m.streaming && !isCurrentlyStreaming) {
        m.streaming = false;
        m.paused = true;
        m.statusText = 'Paused';
        const lastToolEnd = Array.isArray(m.toolCalls)
          ? m.toolCalls.reduce((mx: number, tc: any) => Math.max(mx, tc.finishedAt || tc.startedAt || 0), 0)
          : 0;
        const lastThoughtEnd = Array.isArray(m.steps)
          ? m.steps.reduce((mx: number, st: any) => Math.max(mx, st.finishedAt || st.startedAt || 0), 0)
          : 0;
        m.finishedAt = m.finishedAt || Math.max(lastToolEnd, lastThoughtEnd) || m.startedAt || m.createdAt || Date.now();
        this.finalizeRunningTools(m, 'paused');
        changed = true;
      }
    }
    return { msgs: parsedMsgs, changed };
  }

  public setSessionModel(providerId: string, modelId: string, targetChatId?: string): void {
    const cid = targetChatId || this.activeChatId;
    if (this.draftSession && (!cid || cid === this.draftSession.id)) {
      this.draftSession.providerId = providerId;
      this.draftSession.modelId = modelId;
    }
    settingsStore.setProvider(providerId);
    settingsStore.setModel(modelId);
    if (!cid) return;
    const sess = this.sessions.find((s) => s.id === cid);
    if (sess) {
      sess.providerId = providerId;
      sess.modelId = modelId;
      const prov = settingsStore.data.providers[providerId] || settingsStore.currentProvider;
      const caps = prov?.modelCapabilities?.[modelId] || settingsStore.currentModelCapabilities;
      if (this.contextTelemetryByChat[cid]) {
        this.contextTelemetryByChat[cid].contextWindow = caps.contextWindow || 128000;
      }
      void this.saveSessions(cid);
    }
  }

  public recordModelChanged(newModelId: string): void {
    const cid = this.activeChatId;
    if (!cid || !newModelId) return;
    const list = this.messagesByChat[cid] || [];
    if (list.length === 0) return;
    const last = list[list.length - 1];
    if (last.role === 'system' && last.content === newModelId) return;
    const sysMsg: ChatMessage = {
      id: `sys-model-${Date.now()}-${Math.random().toString(36).slice(2, 5)}`,
      chatId: cid,
      role: 'system',
      content: newModelId,
      createdAt: Date.now()
    };
    list.push(sysMsg);
    void this.saveSessions(cid);
  }

  public isChatStreaming(chatId: string): boolean {
    return Boolean(this.streamingChats[chatId]);
  }

  public openFilePreview(path: string, addedTexts?: string[], removedTexts?: string[], fallbackContent?: string): void {
    this.filePreview = { path, addedTexts, removedTexts, fallbackContent };
  }

  public closeFilePreview(): void {
    this.filePreview = null;
  }

  public openImagePreview(src: string, alt?: string): void {
    if (!src) return;
    this.imagePreview = { src, alt };
  }

  public closeImagePreview(): void {
    this.imagePreview = null;
  }

  public async refreshGoalState(targetChatId?: string): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    if (!cid) return;
    try {
      const g = await invoke<any>('get_goal_state', { chatId: cid });
      if (g && g.condition) {
        const startedAt = typeof g.startedAt === 'number' ? g.startedAt : Math.floor(Date.now() / 1000);
        this.goalByChat[cid] = {
          status: g.status || 'idle',
          condition: g.condition,
          turn: g.turn || 1,
          maxTurns: g.maxTurns || 100,
          reason: g.lastReason || null,
          elapsedSecs: Math.max(0, Math.floor(Date.now() / 1000) - startedAt)
        };
      } else {
        this.goalByChat[cid] = null;
      }
    } catch {}
  }

  private ensurePlanSummaryInHistory(cid: string, summary: string): void {
    const clean = summary.trim();
    if (!clean) return;
    const msgs = this.messagesByChat[cid];
    if (!msgs || msgs.length === 0) return;
    for (let i = msgs.length - 1; i >= 0; i--) {
      if (msgs[i].role === 'assistant') {
        if (!msgs[i].content.trim()) {
          msgs[i].content = clean;
          this.saveChatMessagesToStorage(cid, msgs);
        }
        break;
      }
    }
  }

  public async approvePlan(targetChatId?: string): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    if (!cid) return;
    const summary = this.pendingPlanByChat[cid]?.summary || '';
    this.ensurePlanSummaryInHistory(cid, summary);
    this.pendingPlanByChat[cid] = null;
    await this.sendMessage('Plan approved. Execute the plan now.', { targetChatId: cid, forceExecute: true });
  }

  public async requestPlanChanges(feedback: string, targetChatId?: string): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    if (!cid || !feedback.trim()) return;
    const summary = this.pendingPlanByChat[cid]?.summary || '';
    this.ensurePlanSummaryInHistory(cid, summary);
    this.pendingPlanByChat[cid] = null;
    await this.sendMessage(`/plan ${feedback.trim()}`, { targetChatId: cid });
  }

  public rejectPlan(targetChatId?: string): void {
    const cid = targetChatId || this.activeChatId;
    if (!cid) return;
    this.pendingPlanByChat[cid] = null;
  }

  public async clearGoal(targetChatId?: string): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    if (!cid) return;
    try {
      await invoke('clear_goal_state', { chatId: cid });
    } catch {}
    if (this.streamingChats[cid] && this.goalByChat[cid]) {
      this.goalByChat[cid] = {
        ...this.goalByChat[cid]!,
        status: 'cleared',
        reason: 'Goal cleared · finishing current step'
      };
    } else {
      this.goalByChat[cid] = null;
    }
  }

  public hideSideQuestion(): void {
    if (this.sideQuestion) {
      this.sideQuestion.visible = false;
    }
  }

  public clearSideQuestion(): void {
    this.sideQuestionReqId++;
    this.sideQuestion = null;
  }

  public async openDiffReview(
    messageId: string,
    title: string,
    snapshotIds: string[],
    focusPath?: string
  ): Promise<void> {
    this.diffReview = { messageId, title, focusPath, files: [], loading: true };
    try {
      const files = await invoke<TurnFileDiff[]>('get_turn_file_diffs', { snapshotIds });
      if (this.diffReview?.messageId === messageId) {
        this.diffReview = { messageId, title, focusPath, files, loading: false };
      }
    } catch {
      if (this.diffReview?.messageId === messageId) {
        this.diffReview = { messageId, title, focusPath, files: [], loading: false };
      }
    }
  }

  public closeDiffReview(): void {
    this.diffReview = null;
  }

  public addInlineReviewComment(c: Omit<InlineReviewComment, 'id'>): void {
    this.inlineReviewComments.push({
      ...c,
      id: `rev-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`
    });
  }

  public removeInlineReviewComment(id: string): void {
    const idx = this.inlineReviewComments.findIndex((c) => c.id === id);
    if (idx >= 0) this.inlineReviewComments.splice(idx, 1);
  }

  public async sendInlineReviewComments(): Promise<void> {
    if (this.inlineReviewComments.length === 0) return;
    const lines = ['[Inline Code Review]'];
    for (const c of this.inlineReviewComments) {
      lines.push(`- ${c.path}:${c.lineNumber} (\`${c.lineText.trim().slice(0, 120)}\`): ${c.comment}`);
    }
    this.inlineReviewComments = [];
    await this.sendMessage(lines.join('\n'));
  }

  public async askSideQuestion(rawInput: string): Promise<void> {
    const trimmed = rawInput.trim();
    const q = trimmed.replace(/^\/btw\b/i, '').trim();
    const chatId = this.activeChatId;
    if (!q) {
      if (this.sideQuestion && this.sideQuestion.chatId === chatId) {
        this.sideQuestion.visible = !this.sideQuestion.visible;
      }
      return;
    }

    const reqId = ++this.sideQuestionReqId;
    this.sideQuestion = {
      chatId: chatId || '',
      question: q,
      answer: '',
      streaming: true,
      error: null,
      visible: true
    };

    try {
      const prov = settingsStore.currentProvider;
      const detectedInfo = prov?.modelCapabilities?.[settingsStore.data.activeModel];
      const list = chatId ? this.messagesByChat[chatId] || [] : [];
      const formatToolRec = (rec: any): string => {
        const name = rec.toolName || 'tool';
        const status = rec.status || 'unknown';
        const elapsed = typeof rec.elapsedMs === 'number' ? ` | ${rec.elapsedMs}ms` : '';
        const rawArgs = (typeof rec.rawArgs === 'string' && rec.rawArgs.trim())
          ? rec.rawArgs.trim()
          : (rec.args && Object.keys(rec.args).length > 0 ? JSON.stringify(rec.args, null, 2) : '');
        const lines: string[] = [`[TOOL CALL: ${name} | status=${status}${elapsed}]`];
        if (rawArgs) lines.push(`Arguments:\n${rawArgs.slice(0, 8000)}`);
        if (rec.output) lines.push(`Tool Output:\n${String(rec.output).slice(0, 12000)}`);
        if (rec.error) lines.push(`Tool Error:\n${String(rec.error).slice(0, 3000)}`);
        return lines.join('\n');
      };
      const history = list
        .filter((m) => m.role === 'user' || m.role === 'assistant')
        .map((m) => {
          let text = m.content.trim();
          const parts: string[] = [];
          if (m.reasoningContent && m.reasoningContent.trim()) {
            parts.push(`[Reasoning]\n${m.reasoningContent.trim().slice(0, 3000)}`);
          }
          const seenIds = new Set<string>();
          let hasMsgStep = false;
          if (m.steps && m.steps.length > 0) {
            for (const s of m.steps) {
              if (s.kind === 'message' && s.text.trim()) {
                hasMsgStep = true;
                parts.push(s.text.trim());
              } else if (s.kind === 'tool' && s.record) {
                if (s.record.toolCallId) seenIds.add(s.record.toolCallId);
                parts.push(formatToolRec(s.record));
              } else if (s.kind === 'compact' && s.summary) {
                parts.push(`[Compacted Context]\n${s.summary}`);
              }
            }
          }
          if (m.toolCalls && m.toolCalls.length > 0) {
            for (const tc of m.toolCalls) {
              if (tc.toolCallId && seenIds.has(tc.toolCallId)) continue;
              parts.push(formatToolRec(tc));
            }
          }
          if (text && !hasMsgStep) parts.push(text);
          if (parts.length > 0) text = parts.join('\n\n');
          return { role: m.role, content: text };
        })
        .filter((m) => m.content.length > 0);

      const onEvent = new Channel<any>();
      onEvent.onmessage = (evt: any) => {
        if (reqId !== this.sideQuestionReqId || !this.sideQuestion || this.sideQuestion.chatId !== (chatId || '')) return;
        if (evt.type === 'delta' && typeof evt.content === 'string') {
          this.sideQuestion.answer += evt.content;
        } else if (evt.type === 'done') {
          this.sideQuestion.streaming = false;
        } else if (evt.type === 'error') {
          this.sideQuestion.streaming = false;
          this.sideQuestion.error = evt.message || 'Side question failed';
        }
      };

      const resolvedSideModel = resolveModelForReasoning(prov, settingsStore.data.activeModel, settingsStore.data.reasoningLevel);
      await invoke('ask_side_question', {
        chatId: chatId || 'side-question',
        question: q,
        modelId: resolvedSideModel,
        providerName: prov?.name || null,
        baseUrl: prov?.baseUrl || null,
        apiKey: prov?.apiKey || null,
        apiProtocol: prov?.apiProtocol || 'auto',
        proxyEnabled: Boolean(prov?.proxyEnabled),
        proxyUrl: prov?.proxyUrl || null,
        contextWindow: detectedInfo?.contextWindow || null,
        maxOutputTokens: detectedInfo?.maxOutputTokens || null,
        history,
        onEvent
      });
      if (reqId === this.sideQuestionReqId && this.sideQuestion && this.sideQuestion.streaming) {
        this.sideQuestion.streaming = false;
      }
    } catch (err: any) {
      if (reqId === this.sideQuestionReqId && this.sideQuestion) {
        this.sideQuestion.streaming = false;
        this.sideQuestion.error = err instanceof Error ? err.message : String(err);
      }
    }
  }

  private async init(): Promise<void> {
    await this.loadSessions();
    if (this.disposed) return;
    if (this.sessions.length > 0) {
      if (this.activeChatId && this.sessions.some((s) => s.id === this.activeChatId && !s.archived)) {
        await this.selectSession(this.activeChatId);
      } else {
        const firstActive = this.sessions.find((s) => !s.archived && !s.isDraft);
        if (firstActive) {
          await this.selectSession(firstActive.id);
        }
      }
    }
    await this.pullBackendChats();
    if (this.disposed) return;
    if (this.sessions.length > 0 && !this.activeChatId) {
      const firstActive = this.sessions.find((s) => !s.archived && !s.isDraft);
      if (firstActive) {
        await this.selectSession(firstActive.id);
      }
    }
    import('@tauri-apps/api/event')
      .then(async ({ listen }) => {
        if (this.disposed) return;
        const u1 = await listen<{ chatId?: string }>('telegram-chat-updated', (ev) => {
          void this.pullBackendChats(ev.payload?.chatId);
        });
        if (this.disposed) { u1(); return; }
        this.unlisteners.push(u1);

        const u2 = await listen<{ providerId?: string | null; modelId?: string | null; chatId?: string | null }>(
          'telegram-settings-updated',
          (ev) => {
            const p = ev.payload;
            if (!p) return;
            const targetCid = p.chatId || this.activeChatId;
            if (!targetCid) return;
            const sess = this.sessions.find((s) => s.id === targetCid);
            if (sess) {
              if (p.providerId) sess.providerId = p.providerId;
              if (p.modelId) sess.modelId = p.modelId;
              this.safeSetStorage('omni_chat_sessions', JSON.stringify(this.sessions.filter((s) => !s.isDraft)));
            }
          }
        );
        if (this.disposed) { u2(); return; }
        this.unlisteners.push(u2);

        const u3 = await listen<any>('telegram-chat-stream', (ev) => {
          this.handleTelegramStreamEvent(ev.payload);
        });
        if (this.disposed) { u3(); return; }
        this.unlisteners.push(u3);

        const u4 = await listen<{ chatId?: string; items?: PendingQueueItem[] }>('prompt-queue-updated', (ev) => {
          const p = ev.payload;
          if (!p?.chatId) return;
          this.applyBackendQueue(p.chatId, p.items || []);
        });
        if (this.disposed) { u4(); return; }
        this.unlisteners.push(u4);
      })
      .catch(() => {});
  }

  public syncAllSessionsToBackend(): void {
    for (const sess of this.sessions) {
      if (sess.isDraft || sess.isThemeStudio || this.streamingChats[sess.id]) continue;
      const rawMsgs = this.messagesByChat[sess.id] || [];
      if (rawMsgs.length === 0) continue;
      const msgs = this.buildRichSyncPayload(rawMsgs);
      invoke('sync_chat_context', {
        chatId: sess.id,
        title: sess.title || sess.id,
        messagesJson: JSON.stringify(msgs),
        projectPath: sess.projectPath || null,
        modelId: sess.modelId || settingsStore.data.activeModel || null,
        isPinned: Boolean(sess.pinned)
      }).catch(() => {});
    }
  }

  private handleTelegramStreamEvent(payload: any): void {
    const chatId = payload?.chatId;
    const evtType = payload?.type;
    if (!chatId || !evtType) return;

    let sess = this.sessions.find((s) => s.id === chatId);
    if (evtType === 'start') {
      const userText = (payload?.userText || '').trim();
      const evtModelId = typeof payload?.modelId === 'string' && payload.modelId.trim() ? payload.modelId.trim() : undefined;
      const evtProviderId = typeof payload?.providerId === 'string' && payload.providerId.trim() ? payload.providerId.trim() : undefined;
      if (!sess) {
        sess = {
          id: chatId,
          title: (userText || 'Telegram Chat').slice(0, 40),
          createdAt: Date.now(),
          updatedAt: Date.now(),
          pinned: false,
          providerId: evtProviderId,
          modelId: evtModelId,
          messageCount: 1,
          preview: userText.slice(0, 80)
        };
        this.sessions.unshift(sess);
      } else {
        sess.hasError = false;
        sess.isPaused = false;
        sess.hasUnreadCompletion = false;
        sess.updatedAt = Date.now();
        if (evtProviderId) sess.providerId = evtProviderId;
        if (evtModelId) sess.modelId = evtModelId;
        if (userText) sess.preview = userText.slice(0, 80);
      }
      this.streamingChats[chatId] = true;
      if (!this.messagesByChat[chatId]) this.messagesByChat[chatId] = [];
      const list = this.messagesByChat[chatId];
      const lastMsg = list[list.length - 1];
      if (userText && (!lastMsg || lastMsg.role !== 'user' || lastMsg.content !== userText)) {
        list.push({
          id: `tg-u-${Date.now()}`,
          chatId,
          role: 'user',
          content: userText,
          createdAt: Date.now()
        });
      }
      const tail = list[list.length - 1];
      if (!tail || tail.role !== 'assistant' || !tail.streaming) {
        list.push({
          id: `tg-a-${Date.now()}`,
          chatId,
          role: 'assistant',
          content: '',
          modelId: evtModelId || sess.modelId,
          reasoningContent: '',
          toolCalls: [],
          steps: [],
          startedAt: Date.now(),
          createdAt: Date.now(),
          streaming: true
        });
      } else {
        tail.error = undefined;
        if (evtModelId || sess.modelId) {
          tail.modelId = evtModelId || sess.modelId;
        }
      }
      this.safeSetStorage('omni_chat_sessions', JSON.stringify(this.sessions.filter((s) => !s.isDraft && !s.isThemeStudio)));
      return;
    }

    if (evtType === 'stopped') {
      this.streamingChats[chatId] = false;
      const list = this.messagesByChat[chatId];
      if (list && list.length > 0) {
        const msg = list[list.length - 1];
        if (msg && msg.role === 'assistant') {
          msg.streaming = false;
          msg.statusText = undefined;
          msg.finishedAt = Date.now();
          this.finalizeRunningTools(msg, 'cancelled', 'Stopped from Telegram');
          this.saveLocalOnly(chatId);
        }
      }
      return;
    }

    const list = this.messagesByChat[chatId];
    if (!list || list.length === 0) return;
    const msg = list[list.length - 1];
    if (!msg || msg.role !== 'assistant') return;

    if (evtType === 'thinking' && payload?.delta) {
      this.applyReasoningChunk(chatId, msg, payload.delta);
      return;
    }
    if (evtType === 'content' && payload?.delta) {
      this.applyContentChunk(chatId, msg, payload.delta);
      return;
    }
    if (evtType === 'tool_start' && payload?.id && payload?.name) {
      if (!msg.toolCalls) msg.toolCalls = [];
      if (!msg.steps) msg.steps = [];
      const rec = formatToolExecution({
        id: payload.id,
        name: payload.name,
        arguments: payload.arguments || '',
        status: 'running',
        startedAt: Date.now()
      });
      msg.toolCalls.push(rec);
      msg.steps.push({ kind: 'tool', record: rec });
      this.saveLocalOnly(chatId);
      return;
    }
    if (evtType === 'tool_end' && payload?.id) {
      const nextStatus = payload.success === false ? 'error' : 'success';
      const now = Date.now();
      if (msg.toolCalls) {
        const tc = msg.toolCalls.find((t) => t.toolCallId === payload.id);
        if (tc) {
          tc.status = nextStatus;
          tc.finishedAt = now;
          tc.elapsedMs = payload.elapsedMs;
          tc.output = payload.output ?? tc.output;
          tc.error = payload.error ?? tc.error;
        }
      }
      if (msg.steps) {
        const st = msg.steps.find((s) => s.kind === 'tool' && s.record.toolCallId === payload.id);
        if (st && st.kind === 'tool') {
          st.record.status = nextStatus;
          st.record.finishedAt = now;
          st.record.elapsedMs = payload.elapsedMs;
          st.record.output = payload.output ?? st.record.output;
          st.record.error = payload.error ?? st.record.error;
        }
      }
      this.saveLocalOnly(chatId);
      return;
    }
    if (evtType === 'context_compacted') {
      if (!msg.steps) msg.steps = [];
      msg.steps.push({
        kind: 'compact',
        id: `cmp-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        beforeTokens: payload?.beforeTokens || 0,
        afterTokens: payload?.afterTokens || 0,
        summary: payload?.summary || ''
      });
      return;
    }
    if (evtType === 'task_board_updated') {
      if (this.todoHideTimers[chatId]) {
        clearTimeout(this.todoHideTimers[chatId]);
        delete this.todoHideTimers[chatId];
      }
      const nextTodos = Array.isArray(payload?.items) ? payload.items.map((t: TodoBoardItem) => ({ ...t })) : [];
      msg.todos = nextTodos.map((t: TodoBoardItem) => ({ ...t }));
      this.todosByChat[chatId] = nextTodos;
      this.saveLocalOnly(chatId);
      return;
    }
    if (evtType === 'retry') {
      msg.statusText = `Reconnecting (attempt ${payload?.attempt || 1})...`;
      msg.retryReason = payload?.reason;
      return;
    }
    if (evtType === 'error') {
      msg.error = payload?.error || 'Telegram execution failed';
      msg.streaming = false;
      msg.finishedAt = Date.now();
      this.streamingChats[chatId] = false;
      if (sess) {
        sess.hasError = true;
        sess.updatedAt = Date.now();
      }
      this.saveLocalOnly(chatId);
      return;
    }
    if (evtType === 'done') {
      const evtModelId = typeof payload?.modelId === 'string' && payload.modelId.trim() ? payload.modelId.trim() : undefined;
      const evtProviderId = typeof payload?.providerId === 'string' && payload.providerId.trim() ? payload.providerId.trim() : undefined;
      if (sess) {
        if (evtProviderId) sess.providerId = evtProviderId;
        if (evtModelId) sess.modelId = evtModelId;
        sess.updatedAt = Date.now();
      }
      msg.modelId = evtModelId || msg.modelId || sess?.modelId;
      msg.streaming = false;
      msg.statusText = undefined;
      msg.finishedAt = Date.now();
      this.streamingChats[chatId] = false;
      if (sess && this.activeChatId !== chatId && !msg.error) {
        sess.hasUnreadCompletion = true;
      }
      const tgTodos = this.todosByChat[chatId] || [];
      if (tgTodos.length > 0 && tgTodos.every((t) => t.status === 'completed')) {
        this.todoHideTimers[chatId] = setTimeout(() => {
          this.todosByChat[chatId] = [];
        }, 3500);
      }
      this.saveLocalOnly(chatId);
    }
  }

  public async pullBackendChats(targetChatId?: string): Promise<void> {
    if (this.pullInFlight) {
      if (targetChatId) this.pendingPullTargets.add(targetChatId);
      else this.pendingPullAll = true;
      return;
    }
    this.pullInFlight = true;
    try {
      await this.pullBackendChatsInner(targetChatId);
    } finally {
      this.pullInFlight = false;
      if (this.pendingPullAll) {
        this.pendingPullAll = false;
        this.pendingPullTargets.clear();
        void this.pullBackendChats();
      } else if (this.pendingPullTargets.size > 0) {
        const nextId = this.pendingPullTargets.values().next().value;
        if (nextId) {
          this.pendingPullTargets.delete(nextId);
          void this.pullBackendChats(nextId);
        }
      }
    }
  }

  private async pullBackendChatsInner(targetChatId?: string): Promise<void> {
    try {
      const backendChats = await invoke<
        Array<{
          id: string;
          projectId?: string | null;
          title: string;
          modelId: string;
          isPinned: boolean;
          createdAt: string;
          updatedAt: string;
        }>
      >('list_chats', { projectId: null });
      if (!Array.isArray(backendChats)) return;

      const backendIds = new Set(backendChats.map((c) => c.id));
      let sessionsChanged = false;

      if (targetChatId && !backendIds.has(targetChatId)) {
        const hadIdx = this.sessions.findIndex((s) => s.id === targetChatId);
        if (hadIdx >= 0 && !this.sessions[hadIdx].isDraft) {
          this.sessions.splice(hadIdx, 1);
          delete this.messagesByChat[targetChatId];
          this.cleanupChatResources(targetChatId);
          sessionsChanged = true;
        }
      }

      let newChatPullBudget = 3;
      const newSessionsToAdd: ChatSession[] = [];
      for (const bc of backendChats) {
        const existing = this.sessions.find((s) => s.id === bc.id);
        const bcUpdatedMs = Date.parse(bc.updatedAt) || Date.now();
        let isBrandNew = false;
        if (!existing) {
          isBrandNew = true;
          const newSess: ChatSession = {
            id: bc.id,
            title: bc.title || 'Telegram Chat',
            createdAt: Date.parse(bc.createdAt) || bcUpdatedMs,
            updatedAt: bcUpdatedMs,
            pinned: Boolean(bc.isPinned),
            projectPath: bc.projectId || undefined,
            modelId: bc.modelId || undefined,
            messageCount: 0,
            preview: ''
          };
          newSessionsToAdd.push(newSess);
          sessionsChanged = true;
        } else {
          if (bc.title && existing.title !== bc.title) {
            existing.title = bc.title;
            sessionsChanged = true;
          }
          if (bc.isPinned !== undefined) {
            const bcPinned = Boolean(bc.isPinned);
            const localPinned = Boolean(existing.pinned);
            if (bcPinned !== localPinned) {
              if (bcUpdatedMs >= (existing.updatedAt || 0)) {
                existing.pinned = bcPinned;
                sessionsChanged = true;
              } else {
                invoke('update_chat_pinned', { id: existing.id, isPinned: localPinned }).catch(() => {});
              }
            }
          }
          const nextProj = bc.projectId || undefined;
          if (nextProj !== existing.projectPath && (targetChatId === bc.id || bcUpdatedMs > (existing.updatedAt || 0))) {
            existing.projectPath = nextProj;
            sessionsChanged = true;
          }
          if (bc.modelId && bc.modelId !== existing.modelId && (targetChatId === bc.id || bcUpdatedMs > (existing.updatedAt || 0))) {
            existing.modelId = bc.modelId;
            const curProv = existing.providerId ? settingsStore.data.providers[existing.providerId] : null;
            const curHasModel = curProv && curProv.enabled !== false && (curProv.availableModels.includes(bc.modelId) || findModelGroup(curProv, bc.modelId) !== null);
            if (!curHasModel) {
              const matchingProv = Object.values(settingsStore.data.providers).find(
                (p) => p.enabled !== false && (p.availableModels.includes(bc.modelId) || findModelGroup(p, bc.modelId) !== null)
              );
              if (matchingProv) existing.providerId = matchingProv.id;
            }
            sessionsChanged = true;
          }
        }

        const shouldPullMsgs =
          targetChatId === bc.id ||
          (!targetChatId && bc.id === this.activeChatId && (!this.messagesByChat[bc.id] || bcUpdatedMs > (existing?.updatedAt || 0) + 1000)) ||
          (!targetChatId && isBrandNew && newChatPullBudget > 0);

        if (shouldPullMsgs) {
          if (isBrandNew && !targetChatId) newChatPullBudget--;
          const localMsgs = this.messagesByChat[bc.id] || [];
          const lastLocal = localMsgs[localMsgs.length - 1];
          const isTgStreaming = Boolean(this.streamingChats[bc.id]) && Boolean(lastLocal?.id?.startsWith('tg-'));
          if (this.streamingChats[bc.id] && !(targetChatId === bc.id && isTgStreaming)) {
            continue;
          }

          const hasTrailingIncompleteLocal =
            lastLocal?.role === 'assistant' &&
            (Boolean(lastLocal.paused) ||
              Boolean(lastLocal.streaming) ||
              (lastLocal.content.trim() === '' &&
                ((lastLocal.steps && lastLocal.steps.length > 0) || (lastLocal.toolCalls && lastLocal.toolCalls.length > 0))));

          const dbMsgs = await invoke<
            Array<{
              id: string;
              chatId: string;
              role: 'user' | 'assistant' | 'system' | 'tool';
              content: string;
              modelId?: string | null;
              reasoningContent?: string | null;
              createdAt: string;
            }>
          >('list_messages', { chatId: bc.id });

          if (Array.isArray(dbMsgs)) {
            const lastDb = dbMsgs[dbMsgs.length - 1];
            const changed =
              dbMsgs.length !== localMsgs.length ||
              (lastDb &&
                lastLocal &&
                (lastDb.content !== lastLocal.content ||
                  lastDb.id !== lastLocal.id ||
                  (Boolean(lastDb.modelId) && lastDb.modelId !== lastLocal.modelId)));
            const isTgDoneInDb =
              isTgStreaming &&
              targetChatId === bc.id &&
              lastDb?.role === 'assistant' &&
              Boolean(lastDb.content?.trim()) &&
              (dbMsgs.length > localMsgs.length || lastDb.id !== lastLocal?.id);
            if (isTgDoneInDb) {
              this.streamingChats[bc.id] = false;
            }
            const allowReplace =
              dbMsgs.length > localMsgs.length ||
              (dbMsgs.length === localMsgs.length && changed && !hasTrailingIncompleteLocal);
            if (!this.streamingChats[bc.id] && (allowReplace || isTgDoneInDb)) {
              const dedupedDb: typeof dbMsgs = [];
              for (const m of dbMsgs) {
                const tail = dedupedDb[dedupedDb.length - 1];
                if (tail && tail.role === 'assistant' && m.role === 'assistant' && tail.content.trim() === m.content.trim()) {
                  continue;
                }
                dedupedDb.push(m);
              }
              const usedPrevIds = new Set<string>();
              const converted: ChatMessage[] = dedupedDb.map((m, idx) => {
                let prev = localMsgs.find((lm) => lm.id === m.id && !usedPrevIds.has(lm.id));
                if (!prev && localMsgs[idx]?.role === m.role && localMsgs[idx]?.content.trim() === m.content.trim() && !usedPrevIds.has(localMsgs[idx].id)) {
                  prev = localMsgs[idx];
                }
                if (!prev) {
                  prev = localMsgs.find((lm) => !usedPrevIds.has(lm.id) && lm.role === m.role && lm.content.trim() === m.content.trim());
                }
                if (prev) usedPrevIds.add(prev.id);
                return {
                  id: prev?.id || m.id,
                  chatId: m.chatId,
                  role: m.role,
                  content: m.content,
                  modelId: m.modelId || prev?.modelId || undefined,
                  attachments: prev?.attachments,
                  reasoningContent: prev?.reasoningContent || m.reasoningContent || undefined,
                  toolCalls: prev?.toolCalls,
                  steps: prev?.steps,
                  todos: prev?.todos,
                  statusText: prev?.statusText,
                  startedAt: prev?.startedAt,
                  finishedAt: prev?.finishedAt,
                  paused: prev?.paused,
                  error: prev?.error,
                  tokenUsage: prev?.tokenUsage,
                  createdAt: prev?.createdAt || Date.parse(m.createdAt) || Date.now(),
                  streaming: false
                };
              });
              if (hasTrailingIncompleteLocal && lastLocal && !usedPrevIds.has(lastLocal.id)) {
                converted.push(lastLocal);
              }
              this.messagesByChat[bc.id] = converted;
              this.saveChatMessagesToStorage(bc.id, converted);
              const sess = this.sessions.find((s) => s.id === bc.id) || newSessionsToAdd.find((s) => s.id === bc.id);
              if (sess) {
                sess.messageCount = converted.length;
                if (lastDb && lastDb.content) {
                  sess.preview = lastDb.content.slice(0, 80);
                }
                if (dbMsgs.length > localMsgs.length) {
                  const lastMsgMs = lastDb?.createdAt ? Date.parse(lastDb.createdAt) : 0;
                  sess.updatedAt = Math.max(bcUpdatedMs, Number.isFinite(lastMsgMs) ? lastMsgMs : 0);
                }
                sessionsChanged = true;
              }
            }
          }
        }
      }

      if (newSessionsToAdd.length > 0) {
        this.sessions.unshift(...newSessionsToAdd);
      }

      if (sessionsChanged) {
        this.safeSetStorage('omni_chat_sessions', JSON.stringify(this.sessions.filter((s) => !s.isDraft && !s.isThemeStudio)));
      }
    } catch {}
  }

  private compactMessagesForStorage(msgs: ChatMessage[], forDb = false): ChatMessage[] {
    const recentOutLimit = forDb ? 16000 : 4000;
    const oldOutLimit = forDb ? 4000 : 800;
    const oldReasonLimit = forDb ? 8000 : 1000;
    return msgs.map((m, idx) => {
      const isRecent = idx >= msgs.length - 6;
      const outLimit = isRecent ? recentOutLimit : oldOutLimit;
      const compactedAttachments = m.attachments?.map((a) => ({
        id: a.id,
        name: a.name,
        path: a.path,
        size: a.size,
        kind: a.kind
      }));
      const compactedTools = m.toolCalls?.map((tc) => ({
        ...tc,
        output: tc.output && tc.output.length > outLimit
          ? tc.output.slice(0, outLimit) + '...'
          : tc.output,
        terminalData: tc.terminalData
          ? {
              ...tc.terminalData,
              stdout: tc.terminalData.stdout && tc.terminalData.stdout.length > outLimit
                ? tc.terminalData.stdout.slice(0, outLimit) + '...'
                : tc.terminalData.stdout
            }
          : undefined
      }));
      const compactedSteps = m.steps?.map((s) => {
        if (s.kind !== 'tool') return s;
        return {
          kind: 'tool' as const,
          record: {
            ...s.record,
            output: s.record.output && s.record.output.length > outLimit
              ? s.record.output.slice(0, outLimit) + '...'
              : s.record.output,
            terminalData: s.record.terminalData
              ? {
                  ...s.record.terminalData,
                  stdout: s.record.terminalData.stdout && s.record.terminalData.stdout.length > outLimit
                    ? s.record.terminalData.stdout.slice(0, outLimit) + '...'
                    : s.record.terminalData.stdout
                }
              : undefined
          }
        };
      });
      return {
        ...m,
        attachments: compactedAttachments,
        toolCalls: compactedTools,
        steps: compactedSteps,
        reasoningContent: !isRecent && m.reasoningContent && m.reasoningContent.length > oldReasonLimit
          ? m.reasoningContent.slice(0, oldReasonLimit) + '...'
          : m.reasoningContent
      };
    });
  }

  private safeSetStorage(key: string, value: string, protectChatId?: string): boolean {
    if (typeof localStorage === 'undefined') return false;
    try {
      localStorage.setItem(key, value);
      return true;
    } catch {
      try {
        const candidates = this.sessions
          .filter((s) => s.id !== protectChatId && s.id !== this.activeChatId && !s.pinned)
          .sort((a, b) => (a.updatedAt || 0) - (b.updatedAt || 0));
        for (const s of candidates) {
          localStorage.removeItem(`omni_chat_msgs_${s.id}`);
          try {
            localStorage.setItem(key, value);
            return true;
          } catch {}
        }
      } catch {}
      return false;
    }
  }

  private saveChatMessagesToStorage(chatId: string, msgs: ChatMessage[]): void {
    const raw = JSON.stringify(msgs);
    if (this.safeSetStorage(`omni_chat_msgs_${chatId}`, raw, chatId)) return;
    const compacted = JSON.stringify(this.compactMessagesForStorage(msgs));
    this.safeSetStorage(`omni_chat_msgs_${chatId}`, compacted, chatId);
  }

  private persistDraftsNow(): void {
    this.safeSetStorage(
      'omni_chat_drafts',
      JSON.stringify({
        texts: $state.snapshot(this.draftTextByChat),
        attachments: $state.snapshot(this.draftAttachmentsByChat)
      })
    );
  }

  private scheduleDraftPersist(): void {
    if (this.draftSaveTimer) clearTimeout(this.draftSaveTimer);
    this.draftSaveTimer = setTimeout(() => {
      this.draftSaveTimer = null;
      this.persistDraftsNow();
    }, 350);
  }

  private flushDraftsNow(): void {
    if (this.draftSaveTimer) {
      clearTimeout(this.draftSaveTimer);
      this.draftSaveTimer = null;
      this.persistDraftsNow();
    }
  }

  private persistPendingQueues(): void {
    const json = JSON.stringify($state.snapshot(this.pendingQueueByChat));
    this.safeSetStorage('omni_pending_queues', json);
    invoke('save_ui_pending_queues', { queuesJson: json }).catch(() => {});
  }

  private getDraftKey(chatId: string | null): string {
    if (!chatId) return '__new__';
    const sess = this.sessions.find((s) => s.id === chatId);
    if (!sess || sess.isDraft) return '__new__';
    return chatId;
  }

  public getDraftText(chatId: string | null): string {
    const key = this.getDraftKey(chatId);
    return this.draftTextByChat[key] || '';
  }

  public setDraftText(chatId: string | null, text: string): void {
    const key = this.getDraftKey(chatId);
    if ((this.draftTextByChat[key] || '') === text) return;
    if (!text) {
      delete this.draftTextByChat[key];
    } else {
      this.draftTextByChat[key] = text;
    }
    this.scheduleDraftPersist();
  }

  public getDraftAttachments(chatId: string | null): ChatAttachment[] {
    const key = this.getDraftKey(chatId);
    return this.draftAttachmentsByChat[key] ? [...this.draftAttachmentsByChat[key]] : [];
  }

  public setDraftAttachments(chatId: string | null, atts: ChatAttachment[]): void {
    const key = this.getDraftKey(chatId);
    const clean = atts.map((a) => ({
      id: a.id,
      name: a.name,
      path: a.path,
      size: a.size,
      kind: a.kind,
      previewUrl: a.previewUrl && a.previewUrl.startsWith('data:') ? undefined : a.previewUrl
    }));
    const prev = this.draftAttachmentsByChat[key] || [];
    if (prev.length === clean.length && JSON.stringify(prev) === JSON.stringify(clean)) return;
    if (clean.length === 0) {
      delete this.draftAttachmentsByChat[key];
    } else {
      this.draftAttachmentsByChat[key] = clean;
    }
    this.scheduleDraftPersist();
  }

  public async loadSessions(): Promise<void> {
    if (typeof localStorage !== 'undefined') {
      try {
        const rawDrafts = localStorage.getItem('omni_chat_drafts');
        if (rawDrafts) {
          const parsedDrafts = JSON.parse(rawDrafts);
          if (parsedDrafts && typeof parsedDrafts === 'object') {
            if (parsedDrafts.texts && typeof parsedDrafts.texts === 'object') {
              this.draftTextByChat = parsedDrafts.texts;
            }
            if (parsedDrafts.attachments && typeof parsedDrafts.attachments === 'object') {
              this.draftAttachmentsByChat = parsedDrafts.attachments;
            }
          }
        }
      } catch {}
      try {
        const rawQueues = localStorage.getItem('omni_pending_queues');
        if (rawQueues) {
          const parsedQueues = JSON.parse(rawQueues);
          if (parsedQueues && typeof parsedQueues === 'object') {
            this.pendingQueueByChat = parsedQueues;
          }
        }
      } catch {}
      invoke<string | null>('load_ui_pending_queues')
        .then((dbRaw) => {
          if (!dbRaw) return;
          const dbQueues = JSON.parse(dbRaw);
          if (!dbQueues || typeof dbQueues !== 'object') return;
          let merged = false;
          for (const [k, arr] of Object.entries(dbQueues)) {
            if (Array.isArray(arr) && arr.length > 0 && (!this.pendingQueueByChat[k] || this.pendingQueueByChat[k].length === 0)) {
              this.pendingQueueByChat[k] = arr as PendingQueueItem[];
              merged = true;
            }
          }
          if (merged) {
            this.safeSetStorage('omni_pending_queues', JSON.stringify($state.snapshot(this.pendingQueueByChat)));
          }
        })
        .catch(() => {});
      const saved = localStorage.getItem('omni_chat_sessions');
      if (saved) {
        try {
          const parsed = JSON.parse(saved);
          if (Array.isArray(parsed)) {
            this.sessions = parsed.filter((s) => !s.isDraft && !s.isThemeStudio);
            return;
          }
        } catch {}
      }
    }
    this.sessions = [];
  }

  private saveLocalOnly(targetChatId?: string): void {
    const cid = targetChatId || this.activeChatId;
    const persistableSessions = $state.snapshot(this.sessions).filter((s) => !s.isDraft && !s.isThemeStudio);
    try {
      this.safeSetStorage('omni_chat_sessions', JSON.stringify(persistableSessions), cid || undefined);
      if (cid) {
        const sess = this.sessions.find((s) => s.id === cid);
        if (sess && !sess.isDraft && !sess.isThemeStudio) {
          const msgs = this.messagesByChat[cid];
          if (msgs) {
            this.saveChatMessagesToStorage(cid, $state.snapshot(msgs));
          }
        }
      }
    } catch {}
  }

  public async saveSessions(targetChatId?: string, syncBackend = true, streamingOnly = false): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    this.saveLocalOnly(cid || undefined);
    if (syncBackend && cid) {
      const sess = this.sessions.find((s) => s.id === cid);
      if (sess && !sess.isDraft && !sess.isThemeStudio && this.messagesByChat[cid]) {
        const payload = {
          chatId: cid,
          title: sess.title || cid,
          messagesJson: JSON.stringify(this.buildRichSyncPayload($state.snapshot(this.messagesByChat[cid]))),
          projectPath: sess.projectPath || null,
          modelId: sess.modelId || settingsStore.data.activeModel || null,
          isPinned: Boolean(sess.pinned),
          streamingOnly
        };
        const prev = this.saveChainByChat[cid] || Promise.resolve();
        const next = prev
          .then(async () => { await invoke('sync_chat_context', payload); })
          .catch(() => {});
        this.saveChainByChat[cid] = next;
        await next;
      }
    }
  }

  private syncStreamingCheckpointToDb(chatId: string, minIntervalMs = 5000): void {
    if (this.disposed) return;
    const now = Date.now();
    if (now - (this.lastDbStreamSyncAt[chatId] || 0) < minIntervalMs) return;
    this.lastDbStreamSyncAt[chatId] = now;
    void this.saveSessions(chatId, true, true);
  }

  private finalizeRunningTools(
    msg: ChatMessage,
    targetStatus: 'error' | 'cancelled' | 'paused' | 'success',
    errText?: string
  ): void {
    const now = Date.now();
    if (Array.isArray(msg.steps)) {
      for (const st of msg.steps) {
        if (st.kind === 'thought' && !st.finishedAt) {
          st.finishedAt = now;
        }
      }
    }
    if (Array.isArray(msg.toolCalls)) {
      for (const tc of msg.toolCalls) {
        if (tc.status !== 'running' && tc.status !== 'pending') continue;
        tc.status = targetStatus;
        tc.finishedAt = now;
        tc.elapsedMs = tc.startedAt ? now - tc.startedAt : tc.elapsedMs;
        if (errText !== undefined) tc.error = errText;
      }
    }
    if (Array.isArray(msg.steps)) {
      for (const s of msg.steps) {
        if (s.kind !== 'tool') continue;
        if (s.record.status !== 'running' && s.record.status !== 'pending') continue;
        s.record.status = targetStatus;
        s.record.finishedAt = now;
        s.record.elapsedMs = s.record.startedAt ? now - s.record.startedAt : s.record.elapsedMs;
        if (errText !== undefined) s.record.error = errText;
      }
    }
  }

  private scheduleStreamingSave(chatId: string): void {
    if (this.streamSaveTimers[chatId]) return;
    this.streamSaveTimers[chatId] = window.setTimeout(() => {
      delete this.streamSaveTimers[chatId];
      const runSave = () => {
        if (this.disposed) return;
        this.saveLocalOnly(chatId);
        this.syncStreamingCheckpointToDb(chatId, 12000);
      };
      if (typeof window !== 'undefined' && 'requestIdleCallback' in window) {
        (window as Window & { requestIdleCallback: (cb: () => void, opts?: { timeout: number }) => number }).requestIdleCallback(runSave, { timeout: 2500 });
      } else {
        runSave();
      }
    }, 15000);
  }

  private takeSmoothSlice(buf: string): [string, string] {
    if (buf.length <= 3) return [buf, ''];
    let n = Math.max(2, Math.min(48, Math.ceil(buf.length / 8)));
    if (n < buf.length) {
      const code = buf.charCodeAt(n - 1);
      if (code >= 0xd800 && code <= 0xdbff) {
        n += 1;
      }
    }
    return [buf.slice(0, n), buf.slice(n)];
  }

  private applyReasoningChunk(chatId: string, msg: ChatMessage, chunk: string): void {
    if (!chunk) return;
    msg.statusText = undefined;
    msg.retryReason = undefined;
    msg.reasoningContent = (msg.reasoningContent || '') + chunk;
    if (!msg.steps) msg.steps = [];
    const last = msg.steps[msg.steps.length - 1];
    if (last && last.kind === 'thought') {
      last.text += chunk;
      last.finishedAt = Date.now();
    } else {
      msg.steps.push({
        kind: 'thought',
        id: `th-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        text: chunk,
        startedAt: Date.now(),
        finishedAt: Date.now()
      });
    }
    this.streamTokenCountByChat[chatId] = (this.streamTokenCountByChat[chatId] || 0) + 1;
    this.scheduleStreamingSave(chatId);
  }

  private applyContentChunk(chatId: string, msg: ChatMessage, chunk: string): void {
    if (!chunk) return;
    msg.statusText = undefined;
    msg.retryReason = undefined;
    msg.content += chunk;
    if (!msg.steps) msg.steps = [];
    const last = msg.steps[msg.steps.length - 1];
    if (last && last.kind === 'message') {
      last.text += chunk;
    } else {
      msg.steps.push({
        kind: 'message',
        id: `msg-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        text: chunk
      });
    }
    this.streamTokenCountByChat[chatId] = (this.streamTokenCountByChat[chatId] || 0) + 1;
    this.scheduleStreamingSave(chatId);
  }

  private startSmoothFlush(chatId: string): void {
    if (this.smoothRafByChat[chatId]) return;
    const step = () => {
      this.smoothRafByChat[chatId] = null;
      const list = this.messagesByChat[chatId] || [];
      const targetId = this.activeSmoothMsgByChat[chatId];
      const msg = targetId ? list.find((m) => m.id === targetId) : list[list.length - 1];
      if (!msg || msg.role !== 'assistant') {
        this.pendingReasoningByChat[chatId] = '';
        this.pendingTextByChat[chatId] = '';
        return;
      }
      const rBuf = this.pendingReasoningByChat[chatId] || '';
      const tBuf = this.pendingTextByChat[chatId] || '';
      if (rBuf.length > 0) {
        const [slice, rest] = this.takeSmoothSlice(rBuf);
        this.pendingReasoningByChat[chatId] = rest;
        this.applyReasoningChunk(chatId, msg, slice);
      } else if (tBuf.length > 0) {
        const [slice, rest] = this.takeSmoothSlice(tBuf);
        this.pendingTextByChat[chatId] = rest;
        this.applyContentChunk(chatId, msg, slice);
      }
      if ((this.pendingReasoningByChat[chatId] || '').length > 0 || (this.pendingTextByChat[chatId] || '').length > 0) {
        this.smoothRafByChat[chatId] = requestAnimationFrame(step);
      }
    };
    this.smoothRafByChat[chatId] = requestAnimationFrame(step);
  }

  private flushSmoothBuffers(chatId: string): void {
    const rafId = this.smoothRafByChat[chatId];
    if (rafId) {
      cancelAnimationFrame(rafId);
      this.smoothRafByChat[chatId] = null;
    }
    const list = this.messagesByChat[chatId] || [];
    const targetId = this.activeSmoothMsgByChat[chatId];
    const msg = targetId ? list.find((m) => m.id === targetId) : list[list.length - 1];
    if (msg && msg.role === 'assistant') {
      const rBuf = this.pendingReasoningByChat[chatId] || '';
      const tBuf = this.pendingTextByChat[chatId] || '';
      if (rBuf.length > 0) {
        this.applyReasoningChunk(chatId, msg, rBuf);
      }
      if (tBuf.length > 0) {
        this.applyContentChunk(chatId, msg, tBuf);
      }
    }
    this.pendingReasoningByChat[chatId] = '';
    this.pendingTextByChat[chatId] = '';
  }

  public ensureThemeStudioSession(themeId: string, themeName: string): string {
    const id = `theme-studio-${themeId}`;
    let existing = this.sessions.find((s) => s.id === id);
    if (!existing) {
      existing = {
        id,
        title: `Theme: ${themeName}`,
        createdAt: Date.now(),
        updatedAt: Date.now(),
        pinned: false,
        archived: false,
        isDraft: false,
        isThemeStudio: true,
        themeId,
        providerId: settingsStore.data.activeProvider || undefined,
        modelId: settingsStore.data.activeModel || undefined,
        messageCount: 0,
        preview: ''
      };
      this.sessions.unshift(existing);
    } else {
      existing.themeId = themeId;
    }
    if (!this.messagesByChat[id]) {
      this.messagesByChat[id] = [];
    }
    this.activeChatId = id;
    return id;
  }

  public setActiveSessionProject(path: string): void {
    const cid = this.activeChatId;
    if (!cid) return;
    const sess = this.sessions.find((s) => s.id === cid);
    if (!sess || sess.isThemeStudio) return;
    sess.projectPath = path.trim() ? path.trim() : undefined;
    if (!sess.isDraft) {
      void this.saveSessions(cid);
    }
  }

  public async createSession(title = '', projectPath?: string): Promise<string> {
    const resolvedProject = projectPath !== undefined ? projectPath : workspaceStore.currentPath || undefined;
    const existingDraft = this.sessions.find(
      (s) => s.isDraft && (!this.messagesByChat[s.id] || this.messagesByChat[s.id].length === 0)
    );
    if (existingDraft) {
      if (resolvedProject !== undefined) existingDraft.projectPath = resolvedProject;
      if (title && title !== 'New task') existingDraft.title = title;
      this.activeChatId = existingDraft.id;
      return existingDraft.id;
    }

    const id = `chat-${Date.now()}-${Math.random().toString(36).substring(2, 7)}`;
    const newSession: ChatSession = {
      id,
      title: title && title !== 'New task' ? title : '',
      createdAt: Date.now(),
      updatedAt: Date.now(),
      pinned: false,
      archived: false,
      isDraft: true,
      providerId: settingsStore.data.activeProvider || undefined,
      modelId: settingsStore.data.activeModel || undefined,
      projectPath: resolvedProject,
      hasError: false,
      hasUnreadCompletion: false,
      messageCount: 0,
      preview: ''
    };

    this.showArchived = false;
    this.sessions.unshift(newSession);
    this.messagesByChat[id] = [];
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_active_chat_id', id);
    }
    const provId = newSession.providerId || settingsStore.data.activeProvider;
    const modelId = newSession.modelId || settingsStore.data.activeModel;
    const prov = settingsStore.data.providers[provId] || settingsStore.currentProvider;
    const caps = prov?.modelCapabilities?.[modelId] || settingsStore.currentModelCapabilities;
    this.contextTelemetryByChat[id] = {
      usedTokens: 0,
      contextWindow: caps.contextWindow || 128000,
      cachedTokens: 0,
      cacheHitRate: 0
    };
    this.activeChatId = id;
    return id;
  }

  private scoreMessageList(msgs: ChatMessage[]): number {
    if (!Array.isArray(msgs) || msgs.length === 0) return 0;
    let score = msgs.length * 100000;
    for (const m of msgs) {
      score += (m.content?.length || 0) + (m.reasoningContent?.length || 0);
      score += (m.steps?.length || 0) * 500 + (m.toolCalls?.length || 0) * 500;
    }
    return score;
  }

  private hydrateSnapshotMessages(chatId: string, parsed: any[]): ChatMessage[] {
    const hydrated: ChatMessage[] = parsed.map((m: any, idx: number) => ({
      id: m.id || `cached-${idx}-${Date.now()}`,
      chatId,
      role: m.role || 'assistant',
      content: typeof m.content === 'string' ? m.content : '',
      modelId: m.modelId,
      attachments: m.attachments,
      reasoningContent: m.reasoningContent,
      toolCalls: m.toolCalls,
      steps: m.steps,
      todos: m.todos,
      statusText: m.statusText,
      startedAt: m.startedAt,
      finishedAt: m.finishedAt,
      error: m.error,
      streaming: Boolean(m.streaming),
      paused: Boolean(m.paused),
      tokenUsage: m.tokenUsage,
      createdAt: typeof m.createdAt === 'number' ? m.createdAt : Date.now()
    }));
    return this.normalizeLoadedMessages(chatId, hydrated).msgs;
  }

  private async fetchRecoverySnapshot(chatId: string): Promise<{ msgs: ChatMessage[]; recovered: boolean } | null> {
    try {
      const snap = await invoke<{
        title: string;
        messagesJson: string;
        updatedAt: string;
        recoveredFromCheckpoint: boolean;
      } | null>('get_chat_recovery_snapshot', { chatId });
      if (snap && typeof snap.messagesJson === 'string' && snap.messagesJson.trim()) {
        const parsed = JSON.parse(snap.messagesJson);
        if (Array.isArray(parsed) && parsed.length > 0) {
          return {
            msgs: this.hydrateSnapshotMessages(chatId, parsed),
            recovered: Boolean(snap.recoveredFromCheckpoint)
          };
        }
      }
    } catch {}
    return null;
  }

  public async selectSession(id: string): Promise<void> {
    if (this.activeChatId && this.activeChatId !== id) {
      const prevIdx = this.sessions.findIndex((s) => s.id === this.activeChatId);
      if (prevIdx >= 0) {
        const prev = this.sessions[prevIdx];
        if (prev.isDraft && (!this.messagesByChat[prev.id] || this.messagesByChat[prev.id].length === 0)) {
          this.sessions.splice(prevIdx, 1);
          delete this.messagesByChat[prev.id];
        }
      }
    }
    this.activeChatId = id;
    if (typeof localStorage !== 'undefined') {
      localStorage.setItem('omni_active_chat_id', id);
    }
    const session = this.sessions.find((s) => s.id === id);
    if (session?.projectPath) {
      if (workspaceStore.currentPath !== session.projectPath) {
        workspaceStore.setWorkspaceDir(session.projectPath).catch(() => {});
      }
    } else if (session && !session.isThemeStudio && workspaceStore.currentPath) {
      workspaceStore.clearWorkspace().catch(() => {});
    }
    if (session && session.hasUnreadCompletion) {
      session.hasUnreadCompletion = false;
      this.saveLocalOnly(id);
    }

    void this.refreshGoalState(id);
    if (this.messagesByChat[id]) {
      return;
    }

    if (typeof localStorage !== 'undefined') {
      const saved = localStorage.getItem(`omni_chat_msgs_${id}`);
      if (saved) {
        try {
          const parsedMsgs = JSON.parse(saved);
          if (Array.isArray(parsedMsgs)) {
            const { msgs, changed } = this.normalizeLoadedMessages(id, parsedMsgs);
            this.messagesByChat[id] = msgs;
            if (changed) {
              this.saveChatMessagesToStorage(id, msgs);
            }
            if (!session?.isDraft && !session?.isThemeStudio) {
              this.fetchRecoverySnapshot(id)
                .then((snap) => {
                  if (!snap || this.streamingChats[id]) return;
                  const cur = this.messagesByChat[id] || [];
                  if (this.scoreMessageList(snap.msgs) > this.scoreMessageList(cur) + 20) {
                    this.messagesByChat[id] = snap.msgs;
                    this.saveChatMessagesToStorage(id, snap.msgs);
                    if (snap.recovered) {
                      void this.saveSessions(id, true, false);
                    }
                  }
                })
                .catch(() => {})
                .finally(() => { void this.pullBackendChats(id); });
            }
            return;
          }
        } catch {}
      }
    }

    if (!session?.isDraft && !session?.isThemeStudio) {
      const snap = await this.fetchRecoverySnapshot(id);
      if (snap && snap.msgs.length > 0) {
        this.messagesByChat[id] = snap.msgs;
        this.saveChatMessagesToStorage(id, snap.msgs);
        if (snap.recovered) {
          void this.saveSessions(id, true, false);
        }
        void this.pullBackendChats(id);
        return;
      }
    }

    this.messagesByChat[id] = [];
    if (!session?.isDraft && !session?.isThemeStudio) {
      await this.pullBackendChats(id);
    }
  }

  public async deleteSession(id: string): Promise<void> {
    if (this.streamingChats[id]) {
      await this.stopStreaming(id);
    }
    const idx = this.sessions.findIndex((s) => s.id === id);
    if (idx >= 0) this.sessions.splice(idx, 1);
    delete this.messagesByChat[id];
    this.cleanupChatResources(id);
    this.persistPendingQueues();
    this.persistDraftsNow();

    try {
      await invoke('delete_chat', { id });
    } catch {}

    if (this.activeChatId === id) {
      const next = this.sessions.find((s) => !s.archived && !s.isDraft);
      if (next) {
        await this.selectSession(next.id);
      } else {
        this.activeChatId = null;
      }
    }
    await this.saveSessions();
  }

  public async togglePinSession(id: string): Promise<void> {
    const session = this.sessions.find((s) => s.id === id);
    if (session) {
      session.pinned = !session.pinned;
      invoke('update_chat_pinned', { id, isPinned: session.pinned }).catch(() => {});
      await this.saveSessions(id);
    }
  }

  public async toggleArchiveSession(id: string): Promise<void> {
    const session = this.sessions.find((s) => s.id === id);
    if (session) {
      session.archived = !session.archived;
      if (session.archived) {
        session.pinned = false;
        if (this.activeChatId === id) {
          const next = this.sessions.find((s) => !s.archived && !s.isDraft);
          if (next) {
            await this.selectSession(next.id);
          } else {
            this.activeChatId = null;
          }
        }
      }
      await this.saveSessions(id);
    }
  }

  public async renameSession(id: string, newTitle: string): Promise<void> {
    const session = this.sessions.find((s) => s.id === id);
    if (session && newTitle.trim()) {
      session.title = newTitle.trim();
      await this.saveSessions(id);
    }
  }

  private async runAssistantStream(chatId: string, prompt: string, forceExecute = false): Promise<void> {
    const sess = this.sessions.find((s) => s.id === chatId) || (this.draftSession?.id === chatId ? this.draftSession : null);
    const rawProv = sess?.providerId ? settingsStore.data.providers[sess.providerId] : null;
    const prov = rawProv && rawProv.enabled !== false ? rawProv : settingsStore.currentProvider;
    const rawTargetModel = sess?.modelId || settingsStore.data.activeModel;
    const targetModel = resolveValidModelForProvider(prov, rawTargetModel);

    const assistantMsgId = `asst-${Date.now()}-${Math.random().toString(36).slice(2, 5)}`;
    const assistantMessage: ChatMessage = {
      id: assistantMsgId,
      chatId,
      role: 'assistant',
      content: '',
      modelId: targetModel || settingsStore.data.activeModel || undefined,
      reasoningContent: '',
      toolCalls: [],
      steps: [],
      statusText: 'Working',
      startedAt: Date.now(),
      createdAt: Date.now(),
      streaming: true
    };

    if (!this.messagesByChat[chatId]) this.messagesByChat[chatId] = [];
    this.messagesByChat[chatId].push(assistantMessage);
    this.saveLocalOnly(chatId);
    this.activeStreamMsgIdByChat[chatId] = assistantMsgId;
    this.streamingChats[chatId] = true;
    this.streamTokenCountByChat[chatId] = 0;
    this.reasoningElapsedByChat[chatId] = 0;

    const startTime = Date.now();
    if (this.reasoningTimers[chatId]) {
      clearInterval(this.reasoningTimers[chatId]);
    }
    this.reasoningTimers[chatId] = window.setInterval(() => {
      this.reasoningElapsedByChat[chatId] = Date.now() - startTime;
    }, 500);

    let failed = false;
    try {
      await this.simulateOrExecuteAgentStream(chatId, assistantMsgId, prompt, false, forceExecute);
    } catch (err: any) {
      if (!this.canceledMsgIds.has(assistantMsgId)) {
        failed = true;
        const s = this.sessions.find((x) => x.id === chatId);
        if (s) s.hasError = true;
        const list = this.messagesByChat[chatId] || [];
        const idx = list.findIndex((m) => m.id === assistantMsgId);
        if (idx !== -1) {
          const rawErr = err instanceof Error ? err.message : typeof err === 'string' ? err : 'Connection or streaming error';
          list[idx].error = rawErr;
          list[idx].streaming = false;
          list[idx].paused = false;
          list[idx].finishedAt = Date.now();
          this.finalizeRunningTools(list[idx], 'error', rawErr);
        }
      }
    } finally {
      const isCurrentStream = this.activeStreamMsgIdByChat[chatId] === assistantMsgId;
      if (isCurrentStream) {
        this.activeStreamMsgIdByChat[chatId] = null;
        if (this.reasoningTimers[chatId]) {
          clearInterval(this.reasoningTimers[chatId]);
          delete this.reasoningTimers[chatId];
        }
        if (this.streamSaveTimers[chatId]) {
          clearTimeout(this.streamSaveTimers[chatId]);
          delete this.streamSaveTimers[chatId];
        }
        this.streamingChats[chatId] = false;
      }
      const list = this.messagesByChat[chatId] || [];
      const idx = list.findIndex((m) => m.id === assistantMsgId);
      if (idx !== -1 && !this.canceledMsgIds.has(assistantMsgId)) {
        list[idx].streaming = false;
        if (!list[idx].finishedAt) list[idx].finishedAt = Date.now();
        this.finalizeRunningTools(list[idx], failed ? 'error' : 'success');
      }
      if (isCurrentStream) {
        if (!failed && this.activeChatId !== chatId) {
          const s = this.sessions.find((x) => x.id === chatId);
          if (s) s.hasUnreadCompletion = true;
        }
        await this.saveSessions(chatId);
        await this.processNextInQueue(chatId);
      }
    }
  }

  public async sendMessage(
    text: string,
    options: { reasoningLevel?: ReasoningLevel; files?: string[]; attachments?: ChatAttachment[]; targetChatId?: string; forceExecute?: boolean } = {}
  ): Promise<void> {
    const hasAttachments = Boolean(options.attachments && options.attachments.length > 0);
    if (!text.trim() && !hasAttachments) {
      return;
    }

    const titleSeed = text.trim() || options.attachments?.[0]?.name || 'Chat';
    const prevActive = this.activeChatId;
    let chatId = options.targetChatId || this.activeChatId;
    if (!chatId) {
      chatId = await this.createSession(titleSeed.slice(0, 36));
    }
    if (prevActive) {
      delete this.draftTextByChat[prevActive];
      delete this.draftAttachmentsByChat[prevActive];
    }
    delete this.draftTextByChat[chatId];
    delete this.draftAttachmentsByChat[chatId];
    delete this.draftTextByChat['__new__'];
    delete this.draftAttachmentsByChat['__new__'];
    this.persistDraftsNow();

    const userMsgId = `user-${Date.now()}-${Math.random().toString(36).slice(2, 5)}`;
    const userMessage: ChatMessage = {
      id: userMsgId,
      chatId,
      role: 'user',
      content: text.trim(),
      attachments: options.attachments,
      createdAt: Date.now(),
      modifiedFiles: options.files
    };

    if (!this.messagesByChat[chatId]) this.messagesByChat[chatId] = [];
    const existingList = this.messagesByChat[chatId];
    for (const m of existingList) {
      const wasCanceled = m.error === 'User canceled execution';
      if (m.paused || wasCanceled) {
        m.paused = false;
        if (wasCanceled) m.error = undefined;
        if (m.statusText === 'Paused') m.statusText = undefined;
      }
    }
    existingList.push(userMessage);

    const currentSession = this.sessions.find((s) => s.id === chatId);
    if (currentSession) {
      currentSession.isDraft = false;
      currentSession.updatedAt = Date.now();
      currentSession.messageCount = existingList.length;
      currentSession.preview = text.trim().slice(0, 80);
      currentSession.hasError = false;
      currentSession.isPaused = false;
      currentSession.hasUnreadCompletion = false;
      if (!currentSession.title || currentSession.title === 'New task' || currentSession.title.startsWith('New task')) {
        currentSession.title = titleSeed.slice(0, 36);
      }
    }

    await this.saveSessions(chatId).catch(() => {});
    await this.runAssistantStream(chatId, text.trim(), Boolean(options.forceExecute));
  }

  private async simulateOrExecuteAgentStream(
    chatId: string,
    messageId: string,
    prompt: string,
    resume = false,
    forceExecute = false
  ): Promise<void> {
    const sess = this.sessions.find((s) => s.id === chatId);
    const rawProv = sess?.providerId ? settingsStore.data.providers[sess.providerId] : null;
    const prov = rawProv && rawProv.enabled !== false ? rawProv : settingsStore.currentProvider;
    const rawTargetModel = sess?.modelId || settingsStore.data.activeModel;
    const targetModel = resolveValidModelForProvider(prov, rawTargetModel);
    if (sess && targetModel && sess.modelId && sess.modelId !== targetModel) {
      sess.modelId = targetModel;
      if (prov) sess.providerId = prov.id;
    }
    const detectedInfo = prov?.modelCapabilities?.[targetModel];
    const resolvedStreamModel = resolveModelForReasoning(prov, targetModel, settingsStore.data.reasoningLevel);
    const resolvedCaps = prov?.modelCapabilities?.[resolvedStreamModel] || detectedInfo;
    const modelCaps = resolvedCaps || settingsStore.currentModelCapabilities;

    await new Promise<void>(async (resolve, reject) => {
      let settled = false;
      const done = () => {
        if (!settled) {
          settled = true;
          this.resolveByChat[chatId] = null;
          resolve();
        }
      };
      this.resolveByChat[chatId] = done;

      const onEvent = new Channel<any>();
      onEvent.onmessage = (event) => {
        if (this.canceledMsgIds.has(messageId)) {
          done();
          return;
        }
        this.handleStreamEvent(event, chatId, messageId);
        if (event?.type === 'done' || event?.type === 'error') {
          done();
        }
      };

      try {
        const list = this.messagesByChat[chatId] || [];
        const rawPrior = list
          .slice(0, Math.max(0, list.length - 2))
          .filter((m) => m.role === 'user' || m.role === 'assistant')
          .map((m) => {
            let text = m.content.trim();
            if (!text && m.steps && m.steps.length > 0) {
              text = m.steps
                .filter((s) => s.kind === 'message')
                .map((s) => (s as { kind: 'message'; text: string }).text)
                .join('\n')
                .trim();
            }
            if (!text && m.role === 'assistant') {
              text = m.paused || m.error ? '[Canceled by user]' : (m.toolCalls && m.toolCalls.length > 0 ? '[Completed tool calls]' : '');
            }
            return { role: m.role, content: text };
          })
          .filter((m) => m.content.length > 0);

        const priorHistory: Array<{ role: string; content: string }> = [];
        for (const item of rawPrior) {
          if (
            item.role === 'user' &&
            priorHistory.length > 0 &&
            priorHistory[priorHistory.length - 1].role === 'user'
          ) {
            priorHistory.push({ role: 'assistant', content: '[Canceled by user]' });
          }
          priorHistory.push(item);
        }

        const lastUserMsg = [...list].reverse().find((m) => m.role === 'user');
        const msgAttachments = (lastUserMsg?.attachments || []).map((a) => ({
          name: a.name,
          path: a.path,
          kind: a.kind,
          data_url: a.previewUrl || null
        }));

        if (isImageGenerationModel(resolvedStreamModel)) {
          const curList = this.messagesByChat[chatId] || [];
          const targetMsg = curList.find((m) => m.id === messageId);
          if (targetMsg) {
            targetMsg.imageGenerating = true;
          }
        }
        await invoke('start_chat_stream', {
          chatId,
          prompt,
          modelId: resolvedStreamModel,
          reasoningEffort: settingsStore.data.reasoningLevel,
          providerName: prov?.name || null,
          baseUrl: prov?.baseUrl || null,
          apiKey: prov?.apiKey || null,
          apiProtocol: prov?.apiProtocol || 'auto',
          proxyEnabled: Boolean(prov?.proxyEnabled),
          proxyUrl: prov?.proxyUrl || null,
          contextWindow: resolvedCaps?.contextWindow || modelCaps.contextWindow || null,
          maxOutputTokens: resolvedCaps?.maxOutputTokens || null,
          supportsReasoning: resolvedCaps?.supportsReasoning ?? modelCaps.supportsReasoning,
          workspaceDir: sess?.projectPath || null,
          planMode: !forceExecute && /(?:^|\s)\/plan(?:\s|$)/i.test(prompt.trim()),
          forceExecute,
          resume,
          history: priorHistory,
          attachments: msgAttachments,
          themeId: sess?.themeId || null,
          onEvent
        });
      } catch (err) {
        if (!settled) {
          settled = true;
          this.resolveByChat[chatId] = null;
          reject(err);
        }
      }
    });
  }

  public async stopStreaming(targetChatId?: string, silentSupersede = false): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    if (!cid) return;
    this.flushSmoothBuffers(cid);
    this.activeStreamMsgIdByChat[cid] = null;
    this.streamingChats[cid] = false;
    const resFn = this.resolveByChat[cid];
    if (resFn) {
      resFn();
    }
    if (this.reasoningTimers[cid]) {
      clearInterval(this.reasoningTimers[cid]);
      delete this.reasoningTimers[cid];
    }
    if (this.streamSaveTimers[cid]) {
      clearTimeout(this.streamSaveTimers[cid]);
      delete this.streamSaveTimers[cid];
    }
    const list = this.messagesByChat[cid] || [];
    for (const msg of list) {
      if (msg.streaming) {
        this.canceledMsgIds.add(msg.id);
        msg.streaming = false;
        msg.paused = false;
        msg.statusText = undefined;
        msg.finishedAt = Date.now();
        if (silentSupersede) {
          msg.error = undefined;
        } else if (!msg.error) {
          msg.error = 'User canceled execution';
        }
        this.finalizeRunningTools(msg, 'cancelled', 'User canceled execution');
      }
    }
    try {
      await invoke('cancel_stream', { sessionId: cid });
    } catch {}
    await this.saveSessions(cid);
  }

  public async continueAssistantMessage(messageId?: string): Promise<void> {
    const chatId = this.activeChatId;
    if (!chatId) return;
    const list = this.messagesByChat[chatId] || [];
    if (list.length === 0) return;

    if (!messageId) {
      const lastMsg = list[list.length - 1];
      if (lastMsg.role === 'user' && !this.streamingChats[chatId]) {
        await this.runAssistantStream(chatId, lastMsg.content, false);
        return;
      }
    }

    let targetIdx = -1;
    if (messageId) {
      targetIdx = list.findIndex((m) => m.id === messageId);
      if (targetIdx !== -1 && list[targetIdx].role === 'user' && !this.streamingChats[chatId]) {
        await this.runAssistantStream(chatId, list[targetIdx].content, false);
        return;
      }
    } else {
      for (let i = list.length - 1; i >= 0; i--) {
        if (list[i].role === 'assistant') {
          targetIdx = i;
          break;
        }
      }
    }
    if (targetIdx === -1) return;

    const targetMsg = list[targetIdx];
    if (this.streamingChats[chatId] && targetMsg.streaming) return;
    if (this.streamingChats[chatId]) {
      this.streamingChats[chatId] = false;
    }
    let prevUserPrompt = '';
    for (let i = targetIdx - 1; i >= 0; i--) {
      if (list[i].role === 'user') {
        prevUserPrompt = list[i].content;
        break;
      }
    }
    if (!prevUserPrompt) return;

    const prevElapsed =
      targetMsg.startedAt && targetMsg.finishedAt && targetMsg.finishedAt >= targetMsg.startedAt
        ? targetMsg.finishedAt - targetMsg.startedAt
        : 0;
    const hasPriorWork = Boolean(targetMsg.content?.trim()) || Boolean(targetMsg.toolCalls?.length);

    targetMsg.error = undefined;
    targetMsg.paused = false;
    targetMsg.streaming = true;
    targetMsg.statusText = 'Resuming';
    targetMsg.retryReason = undefined;
    const currentSession = this.sessions.find((s) => s.id === chatId) || (this.draftSession?.id === chatId ? this.draftSession : null);
    targetMsg.modelId = targetMsg.modelId || currentSession?.modelId || settingsStore.data.activeModel;
    if (currentSession) {
      currentSession.hasError = false;
      currentSession.isPaused = false;
    }

    this.canceledMsgIds.delete(targetMsg.id);
    this.activeStreamMsgIdByChat[chatId] = targetMsg.id;
    this.streamingChats[chatId] = true;
    this.streamTokenCountByChat[chatId] = 0;
    const startTime = targetMsg.startedAt ?? Date.now();
    if (this.reasoningTimers[chatId]) {
      clearInterval(this.reasoningTimers[chatId]);
    }
    this.reasoningTimers[chatId] = window.setInterval(() => {
      this.reasoningElapsedByChat[chatId] = Date.now() - startTime;
    }, 500);

    const resumePrompt = hasPriorWork
      ? 'Continue from where you stopped without repeating already completed steps or tool calls.'
      : prevUserPrompt;

    let failed = false;
    try {
      await this.simulateOrExecuteAgentStream(chatId, targetMsg.id, resumePrompt, true);
    } catch (err: any) {
      if (!this.canceledMsgIds.has(targetMsg.id)) {
        failed = true;
        if (currentSession) currentSession.hasError = true;
        const rawErr = err instanceof Error ? err.message : typeof err === 'string' ? err : 'Connection or streaming error';
        targetMsg.error = rawErr;
        targetMsg.streaming = false;
        targetMsg.paused = false;
        targetMsg.finishedAt = Date.now();
        this.finalizeRunningTools(targetMsg, 'error', rawErr);
      }
    } finally {
      const isCurrentStream = this.activeStreamMsgIdByChat[chatId] === targetMsg.id;
      if (isCurrentStream) {
        this.activeStreamMsgIdByChat[chatId] = null;
        if (this.reasoningTimers[chatId]) {
          clearInterval(this.reasoningTimers[chatId]);
          delete this.reasoningTimers[chatId];
        }
        if (this.streamSaveTimers[chatId]) {
          clearTimeout(this.streamSaveTimers[chatId]);
          delete this.streamSaveTimers[chatId];
        }
        this.streamingChats[chatId] = false;
      }
      if (!this.canceledMsgIds.has(targetMsg.id)) {
        targetMsg.streaming = false;
        if (!targetMsg.finishedAt) targetMsg.finishedAt = Date.now();
        this.finalizeRunningTools(targetMsg, failed ? 'error' : 'success');
      }
      if (isCurrentStream) {
        const sess = this.sessions.find((s) => s.id === chatId);
        if (sess) {
          sess.updatedAt = Date.now();
        }
        await this.saveSessions(chatId);
        await this.processNextInQueue(chatId);
      }
    }
  }

  public async regenerateAssistantMessage(messageId?: string): Promise<void> {
    const chatId = this.activeChatId;
    if (!chatId || this.streamingChats[chatId]) return;
    const list = this.messagesByChat[chatId] || [];
    let targetIdx = -1;
    if (messageId) {
      targetIdx = list.findIndex((m) => m.id === messageId);
    } else {
      for (let i = list.length - 1; i >= 0; i--) {
        if (list[i].role === 'assistant') {
          targetIdx = i;
          break;
        }
      }
    }
    if (targetIdx === -1) return;

    let prevUserPrompt = '';
    for (let i = targetIdx - 1; i >= 0; i--) {
      if (list[i].role === 'user') {
        prevUserPrompt = list[i].content;
        break;
      }
    }
    if (!prevUserPrompt) return;

    list.splice(targetIdx);
    this.syncTodosFromMessages(chatId);
    const currentSession = this.sessions.find((s) => s.id === chatId);
    if (currentSession) {
      currentSession.hasError = false;
    }
    await this.runAssistantStream(chatId, prevUserPrompt);
  }

  public async editUserMessage(messageId: string, newText: string, rollbackFiles: boolean): Promise<void> {
    const chatId = this.activeChatId;
    if (!chatId || this.streamingChats[chatId]) return;
    const list = this.messagesByChat[chatId] || [];
    const idx = list.findIndex((m) => m.id === messageId);
    if (idx === -1 || !newText.trim()) return;

    const targetMsg = list[idx];
    if (rollbackFiles && targetMsg.snapshotId) {
      await this.rollbackSnapshot(targetMsg.snapshotId);
    }

    targetMsg.content = newText.trim();
    list.splice(idx + 1);
    this.syncTodosFromMessages(chatId);
    const currentSession = this.sessions.find((s) => s.id === chatId);
    if (currentSession) {
      currentSession.hasError = false;
      currentSession.preview = newText.trim().slice(0, 80);
    }
    await this.runAssistantStream(chatId, newText.trim());
  }

  private syncTodosFromMessages(chatId: string): void {
    if (this.todoHideTimers[chatId]) {
      clearTimeout(this.todoHideTimers[chatId]);
      delete this.todoHideTimers[chatId];
    }
    const list = this.messagesByChat[chatId] || [];
    for (let i = list.length - 1; i >= 0; i--) {
      const m = list[i];
      if (Array.isArray(m.todos) && m.todos.length > 0) {
        const allDone = m.todos.every((t) => t.status === 'completed');
        this.todosByChat[chatId] = allDone ? [] : m.todos.map((t) => ({ ...t }));
        return;
      }
    }
    this.todosByChat[chatId] = [];
  }

  public async rollbackSnapshot(snapshotId: string): Promise<void> {
    try {
      await invoke('rollback_file_snapshot', { snapshotId });
    } catch {}
  }

  public async enqueueMessage(
    text: string,
    priority: 'normal' | 'intervention' = 'normal',
    attachments?: ChatAttachment[]
  ): Promise<void> {
    const cid = this.activeChatId;
    const cleanText = text.trim();
    const hasAtts = Boolean(attachments && attachments.length > 0);
    if (!cid || (!cleanText && !hasAtts)) return;
    const item: PendingQueueItem = {
      id: `queue-${Date.now()}-${Math.random().toString(36).substring(2, 6)}`,
      text: cleanText,
      createdAt: Date.now(),
      priority,
      source: 'app',
      attachments: hasAtts ? attachments : undefined
    };
    const cur = this.pendingQueueByChat[cid] || [];
    this.pendingQueueByChat[cid] = priority === 'intervention' ? [item, ...cur] : [...cur, item];
    this.persistPendingQueues();
    try {
      const items = await invoke<PendingQueueItem[]>('queue_add', {
        chatId: cid,
        text: item.text,
        attachments: hasAtts
          ? attachments!.map((a) => ({
              name: a.name,
              path: a.path,
              kind: a.kind,
              dataUrl: a.previewUrl || null
            }))
          : null,
        priority
      });
      this.applyBackendQueue(cid, items);
    } catch {}
  }

  private normalizeQueueAttachments(raw: unknown): ChatAttachment[] | undefined {
    if (!Array.isArray(raw) || raw.length === 0) return undefined;
    return raw.map((att: unknown, idx: number) => {
      const a = (att && typeof att === 'object' ? att : {}) as Record<string, unknown>;
      const kindRaw = typeof a.kind === 'string' ? a.kind : 'file';
      const kind: ChatAttachment['kind'] =
        kindRaw === 'image' || kindRaw === 'video' || kindRaw === 'audio' ? kindRaw : 'file';
      return {
        id: typeof a.id === 'string' && a.id ? a.id : `qatt-${Date.now()}-${idx}`,
        name: typeof a.name === 'string' ? a.name : 'attachment',
        path: typeof a.path === 'string' ? a.path : '',
        kind,
        previewUrl:
          typeof a.previewUrl === 'string'
            ? a.previewUrl
            : typeof a.dataUrl === 'string'
              ? a.dataUrl
              : undefined
      };
    });
  }

  private applyBackendQueue(chatId: string, items: PendingQueueItem[]): void {
    this.pendingQueueByChat[chatId] = (items || []).map((it) => ({
      id: it.id,
      text: it.text,
      createdAt: it.createdAt ?? Date.now(),
      priority: (it.priority as 'normal' | 'intervention') || 'normal',
      source: it.source,
      attachments: this.normalizeQueueAttachments(it.attachments)
    }));
    this.persistPendingQueues();
  }

  public async dequeueMessage(id: string): Promise<void> {
    const cid = this.activeChatId;
    if (!cid) return;
    this.pendingQueueByChat[cid] = (this.pendingQueueByChat[cid] || []).filter((item) => item.id !== id);
    this.persistPendingQueues();
    try {
      const items = await invoke<PendingQueueItem[]>('queue_remove', { chatId: cid, id });
      this.applyBackendQueue(cid, items);
    } catch {}
  }

  public updatePendingItem(id: string, text: string): void {
    const cid = this.activeChatId;
    if (!cid) return;
    const item = (this.pendingQueueByChat[cid] || []).find((i) => i.id === id);
    if (item && text.trim()) {
      item.text = text.trim();
      this.persistPendingQueues();
    }
  }

  public async sendNow(id: string): Promise<void> {
    const cid = this.activeChatId;
    if (!cid) return;
    const item = (this.pendingQueueByChat[cid] || []).find((i) => i.id === id);
    if (!item) return;
    await this.dequeueMessage(id);
    if (this.streamingChats[cid]) await this.stopStreaming(cid, true);
    await this.sendMessage(item.text, { targetChatId: cid });
  }

  public async processNextInQueue(targetChatId?: string): Promise<void> {
    const cid = targetChatId || this.activeChatId;
    if (!cid || this.streamingChats[cid]) return;
    let next: PendingQueueItem | null = null;
    try {
      const taken = await invoke<PendingQueueItem | null>('queue_take_next', { chatId: cid });
      if (taken) {
        next = {
          id: taken.id,
          text: taken.text,
          createdAt: taken.createdAt ?? Date.now(),
          priority: (taken.priority as 'normal' | 'intervention') || 'normal',
          source: taken.source,
          attachments: this.normalizeQueueAttachments(taken.attachments)
        };
        const items = await invoke<PendingQueueItem[]>('queue_list', { chatId: cid });
        this.applyBackendQueue(cid, items);
      }
    } catch {}
    if (!next) {
      const q = this.pendingQueueByChat[cid] || [];
      if (q.length === 0) return;
      next = q[0];
      this.pendingQueueByChat[cid] = q.slice(1);
      this.persistPendingQueues();
    }
    await this.sendMessage(next.text, { targetChatId: cid, attachments: next.attachments });
  }

  public handleStreamEvent(event: any, targetChatId?: string, targetMessageId?: string): void {
    if (!event) return;
    const cid = targetChatId || this.activeChatId;
    if (!cid) return;
    const list = this.messagesByChat[cid] || [];
    const currentAssistantMsg = targetMessageId
      ? list.find((m) => m.id === targetMessageId)
      : list[list.length - 1];
    if (!currentAssistantMsg || currentAssistantMsg.role !== 'assistant') return;

    if (!currentAssistantMsg.steps) currentAssistantMsg.steps = [];
    if (!currentAssistantMsg.toolCalls) currentAssistantMsg.toolCalls = [];

    if (event.type === 'retry_notice') {
      const attempt = event.attempt || 1;
      const maxA = Math.max(5, attempt);
      currentAssistantMsg.statusText = `Retrying (${attempt}/${maxA})...`;
      currentAssistantMsg.retryReason = event.reason || '';
      return;
    }

    if (event.type === 'context_telemetry') {
      this.contextTelemetryByChat[cid] = {
        usedTokens: event.usedTokens || 0,
        contextWindow: event.contextWindow || 128000,
        cachedTokens: event.cachedTokens || 0,
        cacheHitRate: event.cacheHitRate || 0
      };
      return;
    }

    if (event.type === 'plan_approval_required') {
      const summary = event.planSummary || '';
      this.pendingPlanByChat[cid] = { summary };
      if (summary.trim() && !currentAssistantMsg.content.trim()) {
        currentAssistantMsg.content = summary.trim();
      }
      return;
    }

    if (event.type === 'goal_status_updated') {
      if (event.status === 'cleared') {
        this.goalByChat[cid] = null;
      } else {
        this.goalByChat[cid] = {
          status: event.status || 'active',
          condition: event.condition || '',
          turn: event.turn || 1,
          maxTurns: event.maxTurns || 100,
          reason: event.reason || null,
          elapsedSecs: event.elapsedSecs || 0
        };
      }
      return;
    }

    if (event.type === 'task_board_updated') {
      if (this.todoHideTimers[cid]) {
        clearTimeout(this.todoHideTimers[cid]);
        delete this.todoHideTimers[cid];
      }
      const nextTodos = Array.isArray(event.items) ? event.items.map((t: TodoBoardItem) => ({ ...t })) : [];
      currentAssistantMsg.todos = nextTodos.map((t: TodoBoardItem) => ({ ...t }));
      this.todosByChat[cid] = nextTodos;
      return;
    }

    if (event.type === 'context_compacted') {
      currentAssistantMsg.steps.push({
        kind: 'compact',
        id: `cmp-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
        beforeTokens: event.beforeTokens || 0,
        afterTokens: event.afterTokens || 0,
        summary: event.summary || ''
      });
      this.saveLocalOnly(cid);
      return;
    }

    const settleRunningTools = (status: 'success' | 'error') => {
      const curTools = currentAssistantMsg.toolCalls || [];
      if (!curTools.some((tc) => tc.status === 'running' || tc.status === 'pending')) {
        return;
      }
      const now = Date.now();
      for (const tc of curTools) {
        if (tc.status === 'running' || tc.status === 'pending') {
          tc.status = status;
          tc.finishedAt = tc.finishedAt || now;
          tc.elapsedMs = tc.elapsedMs ?? now - tc.startedAt;
        }
      }
      for (const s of currentAssistantMsg.steps || []) {
        if (s.kind === 'tool' && (s.record.status === 'running' || s.record.status === 'pending')) {
          s.record.status = status;
          s.record.finishedAt = s.record.finishedAt || now;
          s.record.elapsedMs = s.record.elapsedMs ?? now - s.record.startedAt;
        }
      }
    };

    if (!this.turnSnapshotByChat[cid]) {
      this.turnSnapshotByChat[cid] = {
        contentLen: currentAssistantMsg.content.length,
        reasoningLen: (currentAssistantMsg.reasoningContent || '').length,
        stepsLen: (currentAssistantMsg.steps || []).length,
        toolsLen: (currentAssistantMsg.toolCalls || []).length
      };
    }

    if (event.type === 'turn_buffer_reset') {
      this.pendingTextByChat[cid] = '';
      this.pendingReasoningByChat[cid] = '';
      const snap = this.turnSnapshotByChat[cid];
      if (snap) {
        currentAssistantMsg.content = currentAssistantMsg.content.slice(0, snap.contentLen);
        currentAssistantMsg.reasoningContent = (currentAssistantMsg.reasoningContent || '').slice(0, snap.reasoningLen);
        currentAssistantMsg.steps = (currentAssistantMsg.steps || []).slice(0, snap.stepsLen);
        currentAssistantMsg.toolCalls = (currentAssistantMsg.toolCalls || []).slice(0, snap.toolsLen);
      }
      currentAssistantMsg.toolCalls = (currentAssistantMsg.toolCalls || []).filter(
        (tc) => !isInternalArtifactTool(tc.toolName, tc.rawArgs || tc.args)
      );
      currentAssistantMsg.steps = (currentAssistantMsg.steps || []).filter(
        (s) => s.kind !== 'tool' || !isInternalArtifactTool(s.record.toolName, s.record.rawArgs || s.record.args)
      );
      return;
    }

    if (event.type === 'delta') {
      currentAssistantMsg.imageGenerating = false;
      settleRunningTools('success');
      const chunk = event.content ?? event.text ?? '';
      if (chunk) {
        this.activeSmoothMsgByChat[cid] = currentAssistantMsg.id;
        this.pendingTextByChat[cid] = (this.pendingTextByChat[cid] || '') + chunk;
        this.startSmoothFlush(cid);
      }
    } else if (event.type === 'reasoning_delta' || event.type === 'reasoning') {
      settleRunningTools('success');
      const chunk = event.content ?? event.text ?? '';
      if (chunk) {
        this.activeSmoothMsgByChat[cid] = currentAssistantMsg.id;
        this.pendingReasoningByChat[cid] = (this.pendingReasoningByChat[cid] || '') + chunk;
        this.startSmoothFlush(cid);
      }
    } else if (event.type === 'retroactive_reasoning_move') {
      const text = event.reasoningContent || '';
      currentAssistantMsg.reasoningContent = text;
      if (text) {
        currentAssistantMsg.steps.push({
          kind: 'thought',
          id: `th-${Date.now()}-${Math.random().toString(36).slice(2, 6)}`,
          text,
          startedAt: Date.now(),
          finishedAt: Date.now()
        });
      }
    } else if (event.type === 'tool_call_delta') {
      if (isInternalArtifactTool(event.name, event.argumentsDelta || event.arguments)) return;
      this.flushSmoothBuffers(cid);
      if (!event.id) return;
      currentAssistantMsg.statusText = undefined;
      currentAssistantMsg.retryReason = undefined;
      const existingIdx = currentAssistantMsg.toolCalls.findIndex((c) => c.toolCallId === event.id);
      if (existingIdx === -1) {
        const record = formatToolExecution({
          id: event.id,
          name: event.name || 'tool',
          arguments: event.argumentsDelta || '',
          status: 'running',
          startedAt: Date.now()
        });
        currentAssistantMsg.toolCalls.push(record);
        currentAssistantMsg.steps.push({ kind: 'tool', record });
      }
    } else if (event.type === 'tool_call_complete' || event.type === 'tool_start') {
      if (isInternalArtifactTool(event.name, event.arguments)) return;
      this.flushSmoothBuffers(cid);
      currentAssistantMsg.statusText = undefined;
      currentAssistantMsg.retryReason = undefined;
      const existingIdx = currentAssistantMsg.toolCalls.findIndex((c) => c.toolCallId === event.id);
      const startedAt = existingIdx >= 0 ? currentAssistantMsg.toolCalls[existingIdx].startedAt : Date.now();
      const record = formatToolExecution({
        id: event.id,
        name: event.name,
        arguments: event.arguments,
        status: 'running',
        startedAt
      });
      if (existingIdx >= 0) {
        currentAssistantMsg.toolCalls[existingIdx] = record;
        const stepIdx = currentAssistantMsg.steps.findIndex((s) => s.kind === 'tool' && s.record.toolCallId === event.id);
        if (stepIdx >= 0) currentAssistantMsg.steps[stepIdx] = { kind: 'tool', record };
      } else {
        currentAssistantMsg.toolCalls.push(record);
        currentAssistantMsg.steps.push({ kind: 'tool', record });
      }
      this.saveLocalOnly(cid);
    } else if (event.type === 'tool_output_delta') {
      const chunk = event.chunk || '';
      if (!chunk) return;
      const tc = currentAssistantMsg.toolCalls.find((t) => t.toolCallId === event.id);
      if (tc) {
        tc.output = (tc.output || '') + chunk;
        if (tc.terminalData) {
          tc.terminalData.stdout = (tc.terminalData.stdout || '') + chunk;
        }
      }
      const st = currentAssistantMsg.steps.find((s) => s.kind === 'tool' && s.record.toolCallId === event.id);
      if (st && st.kind === 'tool' && st.record !== tc) {
        st.record.output = (st.record.output || '') + chunk;
        if (st.record.terminalData) {
          st.record.terminalData.stdout = (st.record.terminalData.stdout || '') + chunk;
        }
      }
    } else if (event.type === 'tool_end') {
      const now = Date.now();
      const isErr = event.success === false || Boolean(event.error);
      const tc = currentAssistantMsg.toolCalls.find((t) => t.toolCallId === event.id);
      if (tc) {
        tc.status = isErr ? 'error' : 'success';
        tc.finishedAt = now;
        tc.elapsedMs = typeof event.elapsedMs === 'number' ? event.elapsedMs : now - tc.startedAt;
        tc.output = event.output;
        tc.error = event.error;
      }
      const st = currentAssistantMsg.steps.find((s) => s.kind === 'tool' && s.record.toolCallId === event.id);
      if (st && st.kind === 'tool' && st.record !== tc) {
        st.record.status = isErr ? 'error' : 'success';
        st.record.finishedAt = now;
        st.record.elapsedMs = typeof event.elapsedMs === 'number' ? event.elapsedMs : now - st.record.startedAt;
        st.record.output = event.output;
        st.record.error = event.error;
      }
      this.turnSnapshotByChat[cid] = {
        contentLen: currentAssistantMsg.content.length,
        reasoningLen: (currentAssistantMsg.reasoningContent || '').length,
        stepsLen: (currentAssistantMsg.steps || []).length,
        toolsLen: (currentAssistantMsg.toolCalls || []).length
      };
      this.saveLocalOnly(cid);
      this.syncStreamingCheckpointToDb(cid, 5000);
    } else if (event.type === 'done') {
      currentAssistantMsg.imageGenerating = false;
      delete this.turnSnapshotByChat[cid];
      settleRunningTools('success');
      this.flushSmoothBuffers(cid);
      this.streamingChats[cid] = false;
      if (this.reasoningTimers[cid]) {
        clearInterval(this.reasoningTimers[cid]);
        delete this.reasoningTimers[cid];
      }
      if (this.streamSaveTimers[cid]) {
        clearTimeout(this.streamSaveTimers[cid]);
        delete this.streamSaveTimers[cid];
      }
      if (event.usage) {
        currentAssistantMsg.tokenUsage = {
          promptTokens: event.usage.prompt_tokens || event.usage.promptTokens || 0,
          completionTokens: event.usage.completion_tokens || event.usage.completionTokens || 0,
          reasoningTokens: event.usage.reasoning_tokens || event.usage.reasoningTokens
        };
      }
      currentAssistantMsg.finishedAt = Date.now();
      currentAssistantMsg.statusText = undefined;
      currentAssistantMsg.retryReason = undefined;
      const sessForDone = this.sessions.find((s) => s.id === cid) || (this.draftSession?.id === cid ? this.draftSession : null);
      currentAssistantMsg.modelId = currentAssistantMsg.modelId || sessForDone?.modelId || settingsStore.data.activeModel;
      currentAssistantMsg.streaming = false;
      currentAssistantMsg.paused = false;
      this.finalizeRunningTools(currentAssistantMsg, 'success');
      const todos = this.todosByChat[cid] || [];
      if (todos.length > 0 && todos.every((t) => t.status === 'completed')) {
        this.todoHideTimers[cid] = setTimeout(() => {
          this.todosByChat[cid] = [];
        }, 3500);
      }
      void this.saveSessions(cid);
    } else if (event.type === 'error') {
      currentAssistantMsg.imageGenerating = false;
      this.flushSmoothBuffers(cid);
      this.streamingChats[cid] = false;
      if (this.reasoningTimers[cid]) {
        clearInterval(this.reasoningTimers[cid]);
        delete this.reasoningTimers[cid];
      }
      if (this.streamSaveTimers[cid]) {
        clearTimeout(this.streamSaveTimers[cid]);
        delete this.streamSaveTimers[cid];
      }
      currentAssistantMsg.finishedAt = Date.now();
      currentAssistantMsg.statusText = undefined;
      currentAssistantMsg.error = event.message || event.error || 'Stream error';
      currentAssistantMsg.streaming = false;
      currentAssistantMsg.paused = false;
      const isCancel = currentAssistantMsg.error === 'User canceled execution';
      this.finalizeRunningTools(currentAssistantMsg, isCancel ? 'cancelled' : 'error', currentAssistantMsg.error);
      const s = this.sessions.find((x) => x.id === cid);
      if (s) {
        s.hasError = !isCancel;
        s.isPaused = false;
      }
      void this.saveSessions(cid);
    }
  }
}

export const chatStore = new ChatStore();
