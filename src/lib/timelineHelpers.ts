import { convertFileSrc } from '@tauri-apps/api/core';
import type { ToolExecutionRecord } from './toolMapper';

export interface EditTextsMeta {
  filePath: string;
  addedTexts: string[];
  removedTexts: string[];
  added: number;
  removed: number;
}

export interface ToolLabelInfo {
  verb: string;
  target: string;
  added?: number;
  removed?: number;
}

export interface ParsedComputerUseData {
  action: string;
  isListWindows?: boolean;
  windows?: Array<{
    hwnd: number;
    pid: number;
    processName: string;
    title: string;
    className?: string;
    bounds?: { x: number; y: number; width: number; height: number };
    isMinimized?: boolean;
    isOffscreen?: boolean;
    isBound?: boolean;
  }>;
  boundHwnd?: number;
  isBindWindow?: boolean;
  boundInfo?: {
    hwnd: number;
    pid?: number;
    processName?: string;
    title?: string;
    bounds?: { x: number; y: number; width: number; height: number };
    isOffscreen?: boolean;
    isMinimized?: boolean;
    stealthMode?: boolean;
  };
  isSnapshotUi?: boolean;
  elements?: Array<{
    id: string;
    name: string;
    controlType: string;
    automationId?: string;
    bounds?: { x: number; y: number; width: number; height: number };
    enabled?: boolean;
  }>;
  count?: number;
  isInput?: boolean;
  isMultiActions?: boolean;
  multiInfo?: {
    steps: Array<any>;
    count: number;
    delayMs?: number;
  };
  inputInfo?: {
    action: string;
    coords?: { norm_x?: number; norm_y?: number; win_x?: number; win_y?: number };
    text?: string;
    keys?: string;
    status?: string;
    element?: string;
    chars?: number;
  };
  isRestoreWindow?: boolean;
  isLaunchApp?: boolean;
  launchInfo?: {
    pid?: number;
    executable?: string;
    offscreen?: boolean;
    status?: string;
    hwnd?: number;
    title?: string;
  };
  isCloseWindow?: boolean;
  closeInfo?: {
    title?: string;
    processName?: string;
    hwnd?: number;
  };
}

export function formatDuration(diffMs: number, allowMs = false): string {
  if (diffMs < 1000) {
    return allowMs ? `${Math.max(1, Math.round(diffMs))}ms` : '0s';
  }
  const totalSecs = Math.floor(diffMs / 1000);
  const hrs = Math.floor(totalSecs / 3600);
  const mins = Math.floor((totalSecs % 3600) / 60);
  const secs = totalSecs % 60;

  if (hrs > 0) {
    return mins > 0 ? `${hrs}h ${mins}m ${secs}s` : `${hrs}h ${secs}s`;
  }
  if (mins > 0) {
    return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
  }
  return `${secs}s`;
}

export function formatCompactTokens(t: number): string {
  if (t >= 1000) return `${Math.round(t / 1000)}k`;
  return `${t}`;
}

export function formatFriendlyError(raw: string): string {
  if (raw === 'User canceled execution') return raw;
  const statusMatch = raw.match(/HTTP\s*(?:error\s*)?(\d{3})/i);
  const statusPrefix = statusMatch ? `HTTP ${statusMatch[1]}: ` : '';
  const braceIdx = raw.indexOf('{');
  if (braceIdx !== -1) {
    try {
      const parsed = JSON.parse(raw.slice(braceIdx));
      const msg = parsed?.error?.message || parsed?.message || parsed?.detail;
      if (typeof msg === 'string' && msg.trim()) {
        return `${statusPrefix}${msg.trim()}`;
      }
    } catch {}
    return raw.slice(0, braceIdx).replace(/[:\s]+$/, '') || 'Request failed';
  }
  return raw.split('\n')[0] || raw;
}

const EDIT_TEXTS_CACHE_MAX = 2000;
const editTextsCache = new Map<string, { sig: string; value: EditTextsMeta }>();

function countLines(str: string): number {
  let lines = 1;
  for (let i = 0; i < str.length; i++) {
    if (str.charCodeAt(i) === 10) lines++;
  }
  return lines;
}

export function extractEditTexts(record: ToolExecutionRecord): EditTextsMeta {
  const diffLen = record.diff?.changes ? record.diff.changes.length : -1;
  const editsLen = Array.isArray(record.args?.edits) ? record.args.edits.length : -1;
  const contentLen = typeof record.args?.content === 'string' ? record.args.content.length : -1;
  const filePath = String(record.args?.path || record.args?.Path || record.previewArgs || '');
  const sig = `${filePath}|${diffLen}|${editsLen}|${contentLen}|${record.status}`;

  const cached = editTextsCache.get(record.toolCallId);
  if (cached && cached.sig === sig) {
    return cached.value;
  }

  const addedTexts: string[] = [];
  const removedTexts: string[] = [];
  let added = 0;
  let removed = 0;

  if (record.diff?.changes) {
    for (const c of record.diff.changes) {
      if (c.type === 'add') {
        added++;
        addedTexts.push(c.text);
      } else if (c.type === 'del') {
        removed++;
        removedTexts.push(c.text);
      }
    }
  } else if (Array.isArray(record.args?.edits)) {
    for (const ed of record.args.edits) {
      const oldT = typeof ed?.old_text === 'string' ? ed.old_text : '';
      const newT = typeof ed?.new_text === 'string' ? ed.new_text : '';
      if (oldT) {
        removed += countLines(oldT);
        removedTexts.push(oldT);
      }
      if (newT) {
        added += countLines(newT);
        addedTexts.push(newT);
      }
    }
  } else if (typeof record.args?.content === 'string' && record.toolName.toLowerCase().includes('write')) {
    added += countLines(record.args.content);
    addedTexts.push(record.args.content);
  }

  const value: EditTextsMeta = { filePath, addedTexts, removedTexts, added, removed };
  if (editTextsCache.size >= EDIT_TEXTS_CACHE_MAX) {
    const firstKey = editTextsCache.keys().next().value;
    if (firstKey !== undefined) editTextsCache.delete(firstKey);
  }
  editTextsCache.set(record.toolCallId, { sig, value });
  return value;
}

const TOOL_LABEL_CACHE_MAX = 2000;
const toolLabelCache = new Map<string, { sig: string; value: ToolLabelInfo }>();

export function formatToolLabel(record: ToolExecutionRecord, isStreaming: boolean): ToolLabelInfo {
  const outLen = record.output ? record.output.length : 0;
  const sig = `${record.status}|${outLen}|${isStreaming ? 1 : 0}|${record.previewArgs || ''}`;
  const cached = toolLabelCache.get(record.toolCallId);
  if (cached && cached.sig === sig) {
    return cached.value;
  }

  const value = computeToolLabel(record, isStreaming);
  if (toolLabelCache.size >= TOOL_LABEL_CACHE_MAX) {
    const firstKey = toolLabelCache.keys().next().value;
    if (firstKey !== undefined) toolLabelCache.delete(firstKey);
  }
  toolLabelCache.set(record.toolCallId, { sig, value });
  return value;
}

function computeToolLabel(record: ToolExecutionRecord, isStreaming: boolean): ToolLabelInfo {
  const n = record.toolName.toLowerCase();
  if (n === 'tool_hub' && record.args?.action === 'load_skill') {
    return { verb: 'Skill', target: String(record.args?.name || 'skill') };
  }
  if (n.includes('read')) {
    const { filePath } = extractEditTexts(record);
    const fileName = filePath ? (filePath.split(/[\\/]/).pop() || filePath) : '';
    let rangeSuffix = '';
    let startLine = Number(record.args?.offset ?? record.args?.start_line ?? record.args?.line ?? 0);
    let endLine = Number(record.args?.end_line ?? 0);
    if (record.output && record.output.trimStart().startsWith('{')) {
      try {
        const parsed = JSON.parse(record.output);
        if (typeof parsed?.startLine === 'number') startLine = parsed.startLine;
        if (typeof parsed?.endLine === 'number') endLine = parsed.endLine;
        else if (typeof parsed?.linesRead === 'number' && parsed.linesRead > 0) {
          const s0 = startLine > 0 ? startLine : 1;
          endLine = s0 + parsed.linesRead - 1;
          startLine = s0;
        } else if (typeof parsed?.content === 'string') {
          const lineCnt = countLines(parsed.content);
          const s0 = startLine > 0 ? startLine : 1;
          endLine = s0 + Math.max(0, lineCnt - 1);
          startLine = s0;
        }
      } catch {}
    } else if (startLine > 0 && Number(record.args?.limit) > 0) {
      endLine = startLine + Number(record.args.limit) - 1;
    }
    if (startLine > 0 && endLine >= startLine) {
      rangeSuffix = ` #L${startLine}-${endLine}`;
    }
    return { verb: 'Read', target: `${fileName || record.actionName}${rangeSuffix}` };
  }
  if (record.args?.description && typeof record.args.description === 'string') {
    return { verb: '', target: record.args.description };
  }
  const { filePath, added, removed } = extractEditTexts(record);
  const fileName = filePath ? (filePath.split(/[\\/]/).pop() || filePath) : '';

  if (n === 'generate_image') {
    const isErr = record.status === 'error';
    const isRunning = isStreaming && (record.status === 'running' || record.status === 'pending');
    const p = String(record.args?.prompt || '').trim();
    const shortPrompt = p ? `"${p.slice(0, 48)}${p.length > 48 ? '...' : ''}"` : 'Image';
    return {
      verb: isRunning ? 'Generating' : isErr ? 'Generate' : 'Generated',
      target: shortPrompt
    };
  }
  if (n.includes('write')) {
    const isErr = record.status === 'error';
    const isRunning = isStreaming && (record.status === 'running' || record.status === 'pending');
    return {
      verb: isRunning ? 'Writing' : isErr ? 'Write' : 'Wrote',
      target: fileName || record.actionName,
      added: isErr ? 0 : added,
      removed: 0
    };
  }
  if (n.includes('edit') || n.includes('patch')) {
    const isErr = record.status === 'error';
    const isRunning = isStreaming && (record.status === 'running' || record.status === 'pending');
    return {
      verb: isRunning ? 'Editing' : isErr ? 'Edit' : 'Edited',
      target: fileName || record.actionName,
      added: isErr ? 0 : added,
      removed: isErr ? 0 : removed
    };
  }
  if (n.includes('glob')) {
    return { verb: 'Glob', target: record.args?.pattern || fileName || record.actionName };
  }
  if (n.includes('grep') || n.includes('search')) {
    const q = record.args?.pattern || record.args?.query || fileName;
    return { verb: 'Searched', target: q ? String(q) : record.actionName };
  }
  if (n.includes('terminal_exec') || n.includes('run_command') || n.includes('pty') || n.includes('git')) {
    const cmd = record.args?.command || record.args?.CommandLine || record.previewArgs;
    return { verb: 'Ran', target: cmd ? String(cmd) : record.actionName };
  }
  if (n.includes('computer_use')) {
    const act = String(record.args?.action || 'action');
    let parsedOut: any = null;
    if (record.output && record.output.trimStart().startsWith('{')) {
      try { parsedOut = JSON.parse(record.output); } catch {}
    }
    if (act === 'launch_app') {
      return { verb: 'Launched', target: String(record.args?.app || parsedOut?.executable || 'app') };
    }
    if (act === 'bind_window' || act === 'focus_window' || act === 'attach_window') {
      const targetName = parsedOut?.title || record.args?.window_title || parsedOut?.processName || record.args?.process_name || (record.args?.hwnd ? `HWND ${record.args.hwnd}` : 'window');
      return { verb: 'Bound', target: String(targetName) };
    }
    if (act === 'list_windows') {
      const count = parsedOut?.count ?? parsedOut?.windows?.length;
      return { verb: 'Listed', target: count !== undefined ? `${count} windows` : 'Windows' };
    }
    if (act === 'screenshot') {
      const targetName = record.args?.window_title || parsedOut?.title || 'window';
      return { verb: 'Captured', target: String(targetName) };
    }
    if (act === 'snapshot_ui') {
      const count = parsedOut?.count ?? parsedOut?.elements?.length;
      const f = record.args?.filter;
      const target = f ? `UI: "${f}"` : (count !== undefined ? `${count} UI controls` : 'UI elements');
      return { verb: 'Inspected', target };
    }
    if (act === 'uia_action' || act === 'click_element' || act === 'set_value') {
      const el = record.args?.element_name || record.args?.element_id || record.args?.automation_id || 'element';
      return { verb: 'UIA', target: `${record.args?.uia_op || act} ${el}` };
    }
    if (act === 'type') {
      const t = String(record.args?.text || '').slice(0, 36);
      return { verb: 'Typed', target: `"${t}"` };
    }
    if (act === 'hotkey') {
      return { verb: 'Hotkey', target: String(record.args?.keys || '') };
    }
    if (act === 'multi_actions' || record.toolName === 'multi_actions') {
      const stepsCount = Array.isArray(record.args?.actions) ? record.args.actions.length : (Array.isArray(parsedOut?.steps) ? parsedOut.steps.length : 1);
      return { verb: 'Multi Actions', target: `${stepsCount} action${stepsCount === 1 ? '' : 's'}` };
    }
    if (act.includes('click') || act === 'drag' || act === 'scroll') {
      return { verb: 'Input', target: `${act} (${record.args?.x ?? 500}, ${record.args?.y ?? 500})` };
    }
    if (act === 'restore_window' || act === 'show_window') {
      return { verb: 'Restored', target: String(parsedOut?.title || record.args?.window_title || 'window') };
    }
    if (act === 'close_window') {
      return { verb: 'Closed', target: String(parsedOut?.title || record.args?.window_title || record.args?.process_name || 'window') };
    }
    return { verb: 'Computer Use', target: act };
  }
  return { verb: record.actionName, target: record.previewArgs || '' };
}

export function buildMiniDiffLines(call: ToolExecutionRecord): Array<{ type: 'add' | 'del' | 'same'; text: string }> {
  if (call.diff?.changes && call.diff.changes.length > 0) {
    const changes = call.diff.changes;
    const keep = new Set<number>();
    for (let i = 0; i < changes.length; i++) {
      if (changes[i].type !== 'same') {
        for (let j = Math.max(0, i - 2); j <= Math.min(changes.length - 1, i + 2); j++) {
          keep.add(j);
        }
      }
    }
    if (keep.size > 0) {
      return changes.filter((_, idx) => keep.has(idx));
    }
    return changes;
  }
  const meta = extractEditTexts(call);
  const out: Array<{ type: 'add' | 'del' | 'same'; text: string }> = [];
  const maxPairs = Math.max(meta.removedTexts.length, meta.addedTexts.length);
  for (let i = 0; i < maxPairs; i++) {
    const rem = meta.removedTexts[i];
    const add = meta.addedTexts[i];
    if (rem) {
      for (const l of rem.split('\n')) out.push({ type: 'del', text: l });
    }
    if (add) {
      for (const l of add.split('\n')) out.push({ type: 'add', text: l });
    }
  }
  return out;
}

export function parseComputerUse(call: ToolExecutionRecord): ParsedComputerUseData | null {
  if (!call.toolName.toLowerCase().includes('computer_use')) return null;
  const raw = (call.output || '').trim();
  if (!raw.startsWith('{') && !raw.startsWith('[')) return null;
  try {
    const p = JSON.parse(raw);
    const act = String(call.args?.action || p.action || '').toLowerCase();

    if (act === 'list_windows' || Array.isArray(p.windows)) {
      return {
        action: 'list_windows',
        isListWindows: true,
        boundHwnd: typeof p.boundHwnd === 'number' ? p.boundHwnd : undefined,
        count: typeof p.count === 'number' ? p.count : (p.windows?.length || 0),
        windows: Array.isArray(p.windows) ? p.windows : []
      };
    }

    if (act === 'bind_window' || act === 'attach_window' || act === 'focus_window' || p.bound === true) {
      return {
        action: 'bind_window',
        isBindWindow: true,
        boundInfo: {
          hwnd: Number(p.hwnd || call.args?.hwnd || 0),
          pid: p.pid,
          processName: p.processName || call.args?.process_name,
          title: p.title || call.args?.window_title,
          bounds: p.bounds,
          isOffscreen: p.offscreen ?? p.isOffscreen,
          isMinimized: p.isMinimized,
          stealthMode: p.stealthMode
        }
      };
    }

    if (act === 'snapshot_ui' || Array.isArray(p.elements)) {
      return {
        action: 'snapshot_ui',
        isSnapshotUi: true,
        count: typeof p.count === 'number' ? p.count : (p.elements?.length || 0),
        elements: Array.isArray(p.elements) ? p.elements : []
      };
    }

    if (act === 'multi_actions' || call.toolName === 'multi_actions' || Array.isArray(p.steps) || Array.isArray(call.args?.actions)) {
      const steps = Array.isArray(p.steps) ? p.steps : (Array.isArray(call.args?.actions) ? call.args.actions : []);
      return {
        action: 'multi_actions',
        isMultiActions: true,
        multiInfo: {
          steps,
          count: steps.length,
          delayMs: p.delayMs ?? call.args?.delay_ms ?? 150
        }
      };
    }

    if (
      act.includes('click') ||
      act === 'type' ||
      act === 'hotkey' ||
      act === 'scroll' ||
      act === 'drag' ||
      act === 'uia_action' ||
      act === 'set_value'
    ) {
      return {
        action: act,
        isInput: true,
        inputInfo: {
          action: act,
          coords: p.coords,
          text: call.args?.text || p.text,
          keys: call.args?.keys || p.keys,
          status: p.status || (call.status === 'success' ? 'ok' : call.status),
          element: p.element || call.args?.element_name || call.args?.element_id,
          chars: p.chars
        }
      };
    }

    if (act === 'launch_app' || p.executable) {
      return {
        action: 'launch_app',
        isLaunchApp: true,
        launchInfo: {
          pid: p.pid,
          executable: p.executable || call.args?.app,
          offscreen: p.offscreen,
          status: p.status,
          hwnd: p.window?.hwnd || p.hwnd,
          title: p.window?.title || p.title
        }
      };
    }

    if (act === 'restore_window' || act === 'show_window' || p.status === 'restored' || p.status === 'restored_all') {
      return {
        action: 'restore_window',
        isRestoreWindow: true,
        boundInfo: {
          hwnd: p.hwnd,
          title: p.title,
          bounds: p.bounds
        }
      };
    }

    if (act === 'close_window' || p.status === 'close_sent') {
      return {
        action: 'close_window',
        isCloseWindow: true,
        closeInfo: {
          title: p.title,
          processName: p.processName,
          hwnd: p.hwnd
        }
      };
    }

    return null;
  } catch {
    return null;
  }
}

export function extractToolImagePreview(call: ToolExecutionRecord): string | null {
  const raw = (call.output || '').trim();
  if (raw.startsWith('{')) {
    try {
      const parsed = JSON.parse(raw);
      if (typeof parsed?.dataUrl === 'string' && parsed.dataUrl.startsWith('data:image/')) {
        return parsed.dataUrl;
      }
      if (typeof parsed?.previewUrl === 'string' && parsed.previewUrl) {
        return parsed.previewUrl;
      }
      if (typeof parsed?.path === 'string' && /\.(png|jpg|jpeg|webp|gif|bmp)$/i.test(parsed.path)) {
        try { return convertFileSrc(parsed.path); } catch {}
      }
    } catch {}
  }
  const p = typeof call.args?.path === 'string' ? call.args.path.trim() : (typeof call.args?.output_path === 'string' ? call.args.output_path.trim() : '');
  if (p && /\.(png|jpg|jpeg|webp|gif|bmp)$/i.test(p)) {
    try {
      return convertFileSrc(p);
    } catch {}
  }
  return null;
}

export function parseGenerateImage(call: ToolExecutionRecord): {
  prompt: string;
  dataUrl?: string;
  path?: string;
  model?: string;
  aspectRatio?: string;
  bytes?: number;
} | null {
  if (call.toolName !== 'generate_image') return null;
  const prompt = String(call.args?.prompt || '');
  let dataUrl: string | undefined;
  let path: string | undefined = typeof call.args?.output_path === 'string' ? call.args.output_path : undefined;
  let model: string | undefined = typeof call.args?.model === 'string' ? call.args.model : undefined;
  let aspectRatio: string | undefined = typeof call.args?.aspect_ratio === 'string' ? call.args.aspect_ratio : undefined;
  let bytes: number | undefined;

  const raw = (call.output || '').trim();
  if (raw.startsWith('{')) {
    try {
      const p = JSON.parse(raw);
      if (p.dataUrl) dataUrl = p.dataUrl;
      if (p.path) path = p.path;
      if (p.model) model = p.model;
      if (p.aspect_ratio || p.aspectRatio) aspectRatio = p.aspect_ratio || p.aspectRatio;
      if (typeof p.bytes === 'number') bytes = p.bytes;
    } catch {}
  }
  if (!dataUrl && path) {
    try { dataUrl = convertFileSrc(path); } catch {}
  }
  return { prompt, dataUrl, path, model, aspectRatio, bytes };
}

export function parseImageFile(call: ToolExecutionRecord): {
  path: string;
  dataUrl?: string;
  bytes?: number;
} | null {
  const raw = (call.output || '').trim();
  const filePath = String(call.args?.path || call.args?.Path || '');
  const isImgPath = /\.(png|jpg|jpeg|webp|gif|bmp|svg)$/i.test(filePath);
  if (raw.startsWith('{')) {
    try {
      const p = JSON.parse(raw);
      if (p.kind === 'image' || (p.dataUrl && p.dataUrl.startsWith('data:image/'))) {
        return {
          path: p.path || filePath,
          dataUrl: p.dataUrl || (filePath ? convertFileSrc(filePath) : undefined),
          bytes: p.bytes
        };
      }
    } catch {}
  }
  if (isImgPath) {
    return {
      path: filePath,
      dataUrl: convertFileSrc(filePath)
    };
  }
  return null;
}

export function parseWriteFile(call: ToolExecutionRecord): {
  filePath: string;
  content?: string;
  linesCount?: number;
  ext: string;
} | null {
  if (call.toolName !== 'write_file') return null;
  const filePath = String(call.args?.path || call.args?.Path || '');
  const content = typeof call.args?.content === 'string' ? call.args.content : undefined;
  const linesCount = content ? countLines(content) : undefined;
  const ext = filePath.split(/[\\/]/).pop()?.split('.').pop()?.toLowerCase() || 'text';
  return { filePath, content, linesCount, ext };
}

export function parseReadFile(call: ToolExecutionRecord): {
  filePath: string;
  content?: string;
  startLine?: number;
  endLine?: number;
  totalLines?: number;
  ext: string;
} | null {
  if (call.toolName !== 'read_file') return null;
  const filePath = String(call.args?.path || call.args?.Path || '');
  if (/\.(png|jpg|jpeg|webp|gif|bmp|svg)$/i.test(filePath)) return null;
  let content: string | undefined;
  let startLine: number | undefined;
  let endLine: number | undefined;
  let totalLines: number | undefined;

  const raw = (call.output || '').trim();
  if (raw.startsWith('{')) {
    try {
      const p = JSON.parse(raw);
      if (typeof p.content === 'string') content = p.content;
      if (typeof p.startLine === 'number') startLine = p.startLine;
      if (typeof p.endLine === 'number') endLine = p.endLine;
      if (typeof p.totalLines === 'number') totalLines = p.totalLines;
    } catch {}
  }
  if (!content && raw && !raw.startsWith('{')) {
    content = raw;
  }
  const ext = filePath.split(/[\\/]/).pop()?.split('.').pop()?.toLowerCase() || 'text';
  return { filePath, content, startLine, endLine, totalLines, ext };
}

export function formatToolOutputReadable(call: ToolExecutionRecord): string {
  const raw = (call.output || '').trim();
  if (!raw) return '';
  if (call.toolName === 'generate_image') return '';
  if (raw.startsWith('{') || raw.startsWith('[')) {
    try {
      const parsed = JSON.parse(raw);
      if (parsed?.kind === 'image' || parsed?.dataUrl) return '';
      if (typeof parsed?.content === 'string') return parsed.content;
      if (typeof parsed?.output === 'string') return parsed.output;
      if (typeof parsed?.text === 'string') return parsed.text;
      if (typeof parsed?.stdout === 'string') {
        return parsed.stderr ? `${parsed.stdout}\n${parsed.stderr}` : parsed.stdout;
      }
      if (typeof parsed?.markdown === 'string') return parsed.markdown;
      if (typeof parsed?.httpStatus === 'number') {
        const preview = typeof parsed?.htmlPreview === 'string' ? parsed.htmlPreview.trim() : '';
        return preview ? `HTTP ${parsed.httpStatus}\n${preview}` : `HTTP ${parsed.httpStatus}`;
      }
      if (Array.isArray(parsed?.files)) return parsed.files.join('\n');
      if (Array.isArray(parsed?.matches)) {
        return parsed.matches
          .map((m: any) => `${m.file}:${m.line}\n${m.snippet}`)
          .join('\n\n');
      }
      if (Array.isArray(parsed?.items)) {
        return parsed.items.map((it: any) => `[${it.status}] ${it.text}`).join('\n');
      }
      if (Array.isArray(parsed?.windows)) {
        return parsed.windows
          .map(
            (w: any) =>
              `• ${w.title || w.processName} (${w.processName || 'proc'}, PID: ${w.pid}, HWND: ${w.hwnd})${w.isBound ? ' [BOUND]' : ''}${w.isOffscreen ? ' [OFFSCREEN]' : ''}${w.isMinimized ? ' [MINIMIZED]' : ''}`
          )
          .join('\n');
      }
      if (Array.isArray(parsed?.elements)) {
        return parsed.elements
          .map((e: any) => `[${e.id}] ${e.controlType || 'Control'} "${e.name || ''}"${e.automationId ? ` (id: ${e.automationId})` : ''}`)
          .join('\n');
      }
      const searchArr = Array.isArray(parsed) ? parsed : Array.isArray(parsed?.results) ? parsed.results : null;
      if (searchArr) {
        if (searchArr.length === 0) return 'No results found.';
        return searchArr
          .map((item: any) => {
            if (typeof item === 'string') return item;
            if (item && typeof item === 'object') {
              const title = item.title || item.name || '';
              const url = item.url || item.link || '';
              const snippet = item.snippet || item.description || item.content || '';
              if (title || url || snippet) {
                return [title, url, snippet].filter(Boolean).join('\n');
              }
            }
            return JSON.stringify(item);
          })
          .join('\n\n');
      }
      if (typeof parsed?.error === 'string') return parsed.error;
      if (typeof parsed === 'object' && parsed !== null) {
        const hiddenKeys = new Set(['action', 'url', 'snapshotId', 'session_id', 'policy', 'running', 'dataUrl', 'base64', 'system_reminder']);
        const entries = Object.entries(parsed).filter(
          ([k, v]) => !hiddenKeys.has(k) && v !== null && v !== undefined && v !== ''
        );
        if (entries.length === 0) return '';
        if (entries.length === 1) {
          const [, v] = entries[0];
          if (v && typeof v === 'object') {
            return Object.entries(v)
              .map(([sk, sv]) => `${sk}: ${typeof sv === 'object' ? JSON.stringify(sv) : String(sv)}`)
              .join('\n');
          }
          return String(v);
        }
        return entries
          .map(([k, v]) => {
            if (Array.isArray(v)) {
              return `${k}: ${v.map((x) => (typeof x === 'object' ? (x?.name || x?.path || x?.title || JSON.stringify(x)) : String(x))).join(', ')}`;
            }
            if (v && typeof v === 'object') {
              const sub = Object.entries(v)
                .map(([sk, sv]) => `${sk}=${typeof sv === 'object' ? JSON.stringify(sv) : String(sv)}`)
                .join(', ');
              return `${k}: ${sub}`;
            }
            return `${k}: ${String(v)}`;
          })
          .join('\n');
      }
    } catch {}
  }
  return raw;
}
