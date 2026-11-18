export type ToolExecutionStatus = 'pending' | 'running' | 'success' | 'error' | 'cancelled' | 'paused';

export interface DiffLine {
  type: 'add' | 'del' | 'same';
  text: string;
  lineNo?: number;
}

export interface ToolDiff {
  filePath: string;
  before?: string;
  after?: string;
  changes: DiffLine[];
}

export interface TerminalBlock {
  command: string;
  exitCode: number;
  stdout: string;
  stderr: string;
}

export interface ToolExecutionRecord {
  toolCallId: string;
  toolName: string;
  actionName: string;
  icon: string;
  args: Record<string, any>;
  rawArgs: string;
  previewArgs: string;
  status: ToolExecutionStatus;
  startedAt: number;
  finishedAt?: number;
  elapsedMs?: number;
  output?: string;
  error?: string;
  diff?: ToolDiff;
  terminalData?: TerminalBlock;
}

const TOOL_ICONS: Record<string, string> = {
  write_file: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M16 13H8 M16 17H8 M10 9H8',
  edit_file: 'M11 4H4a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h14a2 2 0 0 0 2-2v-7 M18.5 2.5a2.121 2.121 0 0 1 3 3L12 15l-4 1 1-4 9.5-9.5z',
  read_file: 'M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z M14 2v6h6 M12 18v-6 M9 15l3 3 3-3',
  list_directory: 'M22 19a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5l2 3h9a2 2 0 0 1 2 2z',
  file_search: 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35',
  search_files: 'M11 19a8 8 0 1 0 0-16 8 8 0 0 0 0 16z M21 21l-4.35-4.35',
  run_command: 'M4 17l6-6-6-6 M12 19h8',
  terminal_exec: 'M4 17l6-6-6-6 M12 19h8',
  manage_task: 'M4 17l6-6-6-6 M12 19h8',
  terminal_io: 'M4 17l6-6-6-6 M12 19h8',
  pty_spawn: 'M4 17l6-6-6-6 M12 19h8',
  pty_write: 'M4 17l6-6-6-6 M12 19h8',
  git_status: 'M6 3v12 M18 9a3 3 0 1 0 0-6 3 3 0 0 0 0 6z M6 21a3 3 0 1 0 0-6 3 3 0 0 0 0 6z',
  git_diff: 'M16 3h5v5 M4 20L21 3 M21 16v5h-5 M15 15l6 6 M4 4l5 5',
  git_commit: 'M12 3a9 9 0 1 0 0 18 9 9 0 0 0 0-18z M12 7v5l3 3',
  browser_open: 'M12 2a10 10 0 1 0 0 20 10 10 0 0 0 0-20z M2 12h20 M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z',
  browser_click: 'M15 3a6 6 0 0 1 6 6v1a6 6 0 0 1-6 6H9a6 6 0 0 1-6-6V9a6 6 0 0 1 6-6h6z M12 7v4',
  uia_inspect: 'M2 12h20 M12 2v20 M4.93 4.93l14.14 14.14 M4.93 19.07L19.07 4.93',
  uia_click: 'M3 3l7.07 16.97 2.51-7.39 7.39-2.51L3 3z',
  schedule_job: 'M12 2v4 M12 18v4 M4.93 4.93l2.83 2.83 M16.24 16.24l2.83 2.83 M2 12h4 M18 12h4 M4.93 19.07l2.83-2.83 M16.24 7.76l2.83-2.83',
  memory_save: 'M19 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h11l5 5v11a2 2 0 0 1-2 2z M17 21v-8H7v8 M7 3v5h8',
  memory_search: 'M12 2a9 9 0 0 0-9 9c0 3.87 2.45 7.17 5.92 8.41.45.08.62-.2.62-.43v-1.53c-2.42.53-2.93-1.17-2.93-1.17',
  rollback_snapshot: 'M3 12a9 9 0 1 0 9-9 9.75 9.75 0 0 0-6.74 2.74L3 8 M3 3v5h5',
  computer_use: 'M2 4a2 2 0 0 1 2-2h16a2 2 0 0 1 2 2v11a2 2 0 0 1-2 2H4a2 2 0 0 1-2-2V4z M8 21h8 M12 17v4',
  default: 'M14.7 6.3a1 1 0 0 0 0 1.4l1.6 1.6a1 1 0 0 0 1.4 0l3.77-3.77a6 6 0 0 1-7.94 7.94l-6.91 6.91a2.12 2.12 0 0 1-3-3l6.91-6.91a6 6 0 0 1 7.94-7.94l-3.76 3.76z'
};

const ACTION_NAMES: Record<string, string> = {
  write_file: 'Write File',
  edit_file: 'Edit File',
  read_file: 'Read File',
  list_directory: 'List Directory',
  file_search: 'Search Files',
  search_files: 'Search Files',
  run_command: 'Run Shell Command',
  pty_spawn: 'Spawn Terminal',
  pty_write: 'Terminal Stdin',
  git_status: 'Git Status Check',
  git_diff: 'Git Diff Inspection',
  git_commit: 'Git Commit Changes',
  browser_open: 'Open Web Browser',
  browser_click: 'Browser Element Click',
  uia_inspect: 'Inspect UI Automation Tree',
  uia_click: 'Native Windows Click',
  schedule_job: 'Register Scheduled Job',
  memory_save: 'Save System Memory',
  memory_search: 'Query Vector Memory',
  rollback_snapshot: 'Rollback File Snapshot',
  fs_ops: 'Filesystem Op',
  glob: 'Glob Search',
  grep: 'Grep Search',
  code_intel: 'Code Intelligence',
  terminal_exec: 'Run Command',
  manage_task: 'Manage Task',
  terminal_io: 'Terminal I/O',
  inspect_preview: 'Inspect Preview',
  web_research: 'Web Research',
  explore_subagent: 'Explore Subagent',
  subagent_builder: 'Subagent Builder',
  task_board: 'Task Board',
  tool_hub: 'Tool Hub',
  computer_use: 'Computer Use'
};

export function getActionName(toolName: string): string {
  if (ACTION_NAMES[toolName]) {
    return ACTION_NAMES[toolName];
  }
  return toolName
    .replace(/[_-]/g, ' ')
    .replace(/\b\w/g, (char) => char.toUpperCase());
}

export function getToolIcon(toolName: string): string {
  return TOOL_ICONS[toolName] || TOOL_ICONS.default;
}

export function formatArgPreview(args: any, maxLen = 72): string {
  if (!args) {
    return '';
  }
  let preview = '';
  if (typeof args === 'string') {
    preview = args.trim();
  } else if (typeof args === 'object') {
    if (args.path || args.Path) {
      const p = String(args.path || args.Path);
      preview = p;
      if (args.line || args.StartLine) {
        preview += `:${args.line || args.StartLine}`;
      }
    } else if (args.command || args.CommandLine) {
      preview = String(args.command || args.CommandLine);
    } else if (args.action && (args.task_id || args.session_id)) {
      preview = `${args.action} ${args.task_id || args.session_id}`;
    } else if (args.action && (args.app || args.element_id || args.element_name || args.keys || args.window_title || args.process_name)) {
      const detail = args.app || args.element_id || args.element_name || args.keys || args.window_title || args.process_name;
      preview = `${args.action} ${detail}`;
    } else if (args.query || args.pattern) {
      preview = `query: "${args.query || args.pattern}"`;
    } else if (args.url) {
      preview = String(args.url);
    } else if (typeof args.raw === 'string') {
      try {
        const inner = JSON.parse(args.raw);
        if (inner && typeof inner === 'object' && (inner.path || inner.file_path)) {
          preview = String(inner.path || inner.file_path);
        } else {
          preview = args.raw.slice(0, maxLen);
        }
      } catch {
        preview = args.raw.replace(/^\s*\{?"?content"?:\s*"?/, '').slice(0, maxLen);
      }
    } else {
      const keys = Object.keys(args).filter((k) => k !== 'raw');
      const parts = keys.slice(0, 3).map((k) => {
        const val = typeof args[k] === 'object' ? '{...}' : String(args[k]);
        return `${k}: ${val}`;
      });
      preview = parts.join(', ');
    }
  } else {
    preview = String(args);
  }

  preview = preview.replace(/\s+/g, ' ').trim();
  if (preview.length > maxLen) {
    return preview.slice(0, maxLen - 3) + '...';
  }
  return preview;
}

export function formatElapsed(ms: number): string {
  if (!ms || ms < 0) {
    return '0ms';
  }
  if (ms < 1000) {
    return `${Math.round(ms)}ms`;
  }
  const seconds = ms / 1000;
  if (seconds < 60) {
    return `${seconds.toFixed(1)}s`;
  }
  const minutes = Math.floor(seconds / 60);
  const remainingSecs = Math.round(seconds % 60);
  return `${minutes}m ${remainingSecs}s`;
}

export function parseDiff(
  filePath: string,
  before?: string,
  after?: string,
  diffText?: string
): ToolDiff {
  const changes: DiffLine[] = [];

  if (diffText && diffText.includes('@@')) {
    const lines = diffText.split('\n');
    let lineNo = 1;
    for (const line of lines) {
      if (line.startsWith('+++') || line.startsWith('---') || line.startsWith('diff ')) {
        continue;
      }
      if (line.startsWith('@@')) {
        const match = line.match(/\+(\d+)/);
        if (match) {
          lineNo = parseInt(match[1], 10);
        }
        continue;
      }
      if (line.startsWith('+')) {
        changes.push({ type: 'add', text: line.slice(1), lineNo: lineNo++ });
      } else if (line.startsWith('-')) {
        changes.push({ type: 'del', text: line.slice(1) });
      } else {
        changes.push({ type: 'same', text: line.startsWith(' ') ? line.slice(1) : line, lineNo: lineNo++ });
      }
    }
    return { filePath, before, after, changes };
  }

  if (before !== undefined && after !== undefined) {
    const beforeLines = before.split('\n');
    const afterLines = after.split('\n');
    const max = Math.max(beforeLines.length, afterLines.length);

    let bIdx = 0;
    let aIdx = 0;

    while (bIdx < beforeLines.length || aIdx < afterLines.length) {
      const bLine = beforeLines[bIdx];
      const aLine = afterLines[aIdx];

      if (bLine === aLine) {
        changes.push({ type: 'same', text: aLine, lineNo: aIdx + 1 });
        bIdx++;
        aIdx++;
      } else if (bIdx < beforeLines.length && !afterLines.includes(bLine, aIdx)) {
        changes.push({ type: 'del', text: bLine });
        bIdx++;
      } else if (aIdx < afterLines.length) {
        changes.push({ type: 'add', text: aLine, lineNo: aIdx + 1 });
        aIdx++;
      } else {
        changes.push({ type: 'del', text: bLine });
        bIdx++;
      }
    }

    return { filePath, before, after, changes };
  }

  return { filePath, before, after, changes };
}

export function parseTerminalOutput(command: string, rawOutput: string, exitCode = 0): TerminalBlock {
  const cleanOutput = rawOutput
    .replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
    .replace(/\x1b[P^_][\s\S]*?\x1b\\/g, '')
    .replace(/\x1b\[[0-?]*[ -/]*[@-~]/g, '')
    .replace(/\x1b[@-_]/g, '')
    .replace(/\r\n/g, '\n')
    .replace(/^.*\r(?!\n)/gm, '');
  const lines = cleanOutput.split('\n');
  const stdoutLines: string[] = [];
  const stderrLines: string[] = [];

  for (const line of lines) {
    if (line.toLowerCase().startsWith('error:') || line.toLowerCase().startsWith('fatal:')) {
      stderrLines.push(line);
    } else {
      stdoutLines.push(line);
    }
  }

  return {
    command,
    exitCode,
    stdout: stdoutLines.join('\n').trim(),
    stderr: stderrLines.join('\n').trim()
  };
}

export function formatToolExecution(raw: {
  id: string;
  name: string;
  arguments: any;
  status?: ToolExecutionStatus;
  startedAt?: number;
  finishedAt?: number;
  output?: any;
  error?: string;
}): ToolExecutionRecord {
  const toolName = raw.name || 'unknown';
  let parsedArgs: Record<string, any> = {};
  let rawArgsStr = '';

  if (typeof raw.arguments === 'string') {
    rawArgsStr = raw.arguments;
    try {
      parsedArgs = JSON.parse(raw.arguments);
    } catch {
      parsedArgs = { raw: raw.arguments };
    }
  } else if (typeof raw.arguments === 'object' && raw.arguments !== null) {
    parsedArgs = raw.arguments;
    rawArgsStr = JSON.stringify(raw.arguments);
  }

  const previewArgs = formatArgPreview(parsedArgs, 72);
  const startedAt = raw.startedAt || Date.now();
  const finishedAt = raw.finishedAt;
  const elapsedMs = finishedAt ? Math.max(0, finishedAt - startedAt) : undefined;
  const status: ToolExecutionStatus = raw.status || (raw.error ? 'error' : finishedAt ? 'success' : 'running');

  let outputStr: string | undefined;
  if (raw.output !== undefined) {
    outputStr = typeof raw.output === 'string' ? raw.output : JSON.stringify(raw.output, null, 2);
  }

  let diff: ToolDiff | undefined;
  if (toolName === 'edit_file' || toolName === 'write_file' || toolName === 'patch_file' || toolName === 'rollback_snapshot') {
    const filePath = parsedArgs.path || parsedArgs.Path || 'file';
    if (parsedArgs.before !== undefined || parsedArgs.after !== undefined) {
      diff = parseDiff(filePath, parsedArgs.before, parsedArgs.after);
    } else if (parsedArgs.Target && parsedArgs.Replacement) {
      diff = parseDiff(filePath, parsedArgs.Target, parsedArgs.Replacement);
    } else if (outputStr && outputStr.includes('@@')) {
      diff = parseDiff(filePath, undefined, undefined, outputStr);
    }
  }

  let terminalData: TerminalBlock | undefined;
  if (toolName === 'run_command' || toolName === 'pty_spawn' || toolName === 'pty_write') {
    const cmd = parsedArgs.command || parsedArgs.CommandLine || '';
    if (outputStr || raw.error) {
      terminalData = parseTerminalOutput(cmd, outputStr || raw.error || '', raw.error ? 1 : 0);
    }
  }

  return {
    toolCallId: raw.id,
    toolName,
    actionName: getActionName(toolName),
    icon: getToolIcon(toolName),
    args: parsedArgs,
    rawArgs: rawArgsStr,
    previewArgs,
    status,
    startedAt,
    finishedAt,
    elapsedMs,
    output: outputStr,
    error: raw.error,
    diff,
    terminalData
  };
}
