<script lang="ts">
import { invoke } from '@tauri-apps/api/core';

interface ScheduledJob {
  id: string;
  name: string;
  prompt: string;
  cronExpr: string;
  active: boolean;
  nextRun: string;
  lastRun?: string;
  lastStatus?: 'success' | 'failed' | 'running' | 'idle';
  executionCount: number;
}

let jobs = $state<ScheduledJob[]>([
  {
    id: 'job-1',
    name: 'Morning Codebase Health Check',
    prompt: 'Run git status, verify clean working tree, and check test suite passes on main branch',
    cronExpr: '0 9 * * 1-5',
    active: true,
    nextRun: '2026-11-13 09:00',
    lastRun: '2026-11-12 09:00',
    lastStatus: 'success',
    executionCount: 14
  },
  {
    id: 'job-2',
    name: 'Hourly Memory System Compaction',
    prompt: 'Summarize oldest conversation threads and prune low-priority cache entries',
    cronExpr: '0 * * * *',
    active: true,
    nextRun: '2026-11-12 20:00',
    lastRun: '2026-11-12 19:00',
    lastStatus: 'success',
    executionCount: 78
  },
  {
    id: 'job-3',
    name: 'Security Vulnerability Scan',
    prompt: 'Audit package dependencies and check for outdated native dll references',
    cronExpr: '30 2 * * 0',
    active: false,
    nextRun: '2026-11-15 02:30',
    lastRun: '2026-11-08 02:30',
    lastStatus: 'success',
    executionCount: 3
  }
]);

let minute = $state<string>('0');
let hour = $state<string>('9');
let dayOfMonth = $state<string>('*');
let month = $state<string>('*');
let dayOfWeek = $state<string>('1-5');

let showAddModal = $state<boolean>(false);
let newJobName = $state<string>('');
let newJobPrompt = $state<string>('');
let notification = $state<{ type: 'success' | 'error'; message: string } | null>(null);

let composedCron = $derived(
  `${minute.trim() || '*'} ${hour.trim() || '*'} ${dayOfMonth.trim() || '*'} ${month.trim() || '*'} ${dayOfWeek.trim() || '*'}`
);

let cronHumanReadable = $derived.by(() => {
  const c = composedCron;
  if (c === '0 9 * * 1-5') return 'At 09:00 AM, Monday through Friday';
  if (c === '0 * * * *') return 'At the start of every hour';
  if (c === '*/15 * * * *') return 'Every 15 minutes';
  if (c === '0 0 * * *') return 'Every day at midnight (00:00)';
  if (c === '30 2 * * 0') return 'At 02:30 AM on Sunday';
  return `Custom schedule: ${c}`;
});

function applyPreset(preset: '15m' | 'hourly' | 'daily' | 'workdays' | 'weekly') {
  if (preset === '15m') {
    minute = '*/15'; hour = '*'; dayOfMonth = '*'; month = '*'; dayOfWeek = '*';
  } else if (preset === 'hourly') {
    minute = '0'; hour = '*'; dayOfMonth = '*'; month = '*'; dayOfWeek = '*';
  } else if (preset === 'daily') {
    minute = '0'; hour = '0'; dayOfMonth = '*'; month = '*'; dayOfWeek = '*';
  } else if (preset === 'workdays') {
    minute = '0'; hour = '9'; dayOfMonth = '*'; month = '*'; dayOfWeek = '1-5';
  } else if (preset === 'weekly') {
    minute = '30'; hour = '2'; dayOfMonth = '*'; month = '*'; dayOfWeek = '0';
  }
}

async function toggleJobActive(job: ScheduledJob) {
  job.active = !job.active;
  try {
    await invoke('toggle_schedule_job', { jobId: job.id, active: job.active });
    notification = { type: 'success', message: `Job "${job.name}" is now ${job.active ? 'active' : 'paused'}` };
  } catch (err: unknown) {
    notification = { type: 'error', message: `Toggle failed: ${err instanceof Error ? err.message : String(err)}` };
  }
}

async function runJobNow(job: ScheduledJob) {
  job.lastStatus = 'running';
  notification = { type: 'success', message: `Triggered execution for "${job.name}"` };
  try {
    await invoke('execute_scheduled_job_now', { jobId: job.id });
    job.lastStatus = 'success';
    job.lastRun = 'Just now';
    job.executionCount++;
  } catch (err: unknown) {
    job.lastStatus = 'failed';
    notification = { type: 'error', message: `Execution failed: ${err instanceof Error ? err.message : String(err)}` };
  }
}

function handleAddJob() {
  if (!newJobName.trim() || !newJobPrompt.trim()) return;
  const newJob: ScheduledJob = {
    id: 'job-' + Date.now(),
    name: newJobName.trim(),
    prompt: newJobPrompt.trim(),
    cronExpr: composedCron,
    active: true,
    nextRun: 'Calculated upon registration',
    lastStatus: 'idle',
    executionCount: 0
  };
  jobs.push(newJob);
  showAddModal = false;
  newJobName = '';
  newJobPrompt = '';
  notification = { type: 'success', message: `Scheduled job "${newJob.name}" created` };
}

function deleteJob(jobId: string) {
  jobs = jobs.filter((j) => j.id !== jobId);
  notification = { type: 'success', message: 'Scheduled job removed' };
}
</script>

<div class="flex flex-col h-full bg-[#0E0F12] text-[#E5E7EB] font-sans text-xs select-none p-6 space-y-6 overflow-y-auto">
  <div class="flex items-center justify-between pb-4 border-b border-[#1A1D24]">
    <div>
      <h2 class="text-base font-bold text-neutral-100 flex items-center gap-2">
        <span>Autonomous Task Scheduler</span>
        <span class="px-2 py-0.5 rounded text-[10px] uppercase font-semibold bg-emerald-500/15 text-emerald-400 border border-emerald-500/30">
          5-Field Cron Engine
        </span>
      </h2>
      <p class="text-neutral-500 text-[11px] mt-0.5">Automate recurring agent workflows, background maintenance routines, and monitoring tasks</p>
    </div>

    <button
      type="button"
      onclick={() => (showAddModal = true)}
      class="px-4 py-2 bg-blue-600 hover:bg-blue-500 text-white rounded-lg font-medium transition-colors shadow-sm"
    >
      + New Scheduled Task
    </button>
  </div>

  {#if notification}
    <div
      class="p-3 rounded-xl border flex items-center justify-between {notification.type === 'success' ? 'bg-emerald-500/10 border-emerald-500/20 text-emerald-400' : 'bg-rose-500/10 border-rose-500/20 text-rose-400'}"
    >
      <span>{notification.message}</span>
      <button type="button" onclick={() => (notification = null)} class="opacity-70 hover:opacity-100">✕</button>
    </div>
  {/if}

  <div class="p-4 bg-[#131416] rounded-xl border border-[#1A1D24] space-y-4">
    <div class="flex items-center justify-between">
      <div>
        <h3 class="text-sm font-semibold text-neutral-200">Cron Expression Generator</h3>
        <p class="text-neutral-500 text-[11px]">Define execution intervals using standard 5-part POSIX cron syntax</p>
      </div>
      <div class="flex items-center gap-1.5">
        <button
          type="button"
          onclick={() => applyPreset('15m')}
          class="px-2.5 py-1 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded border border-[#2B2E36] text-[11px]"
        >
          Every 15m
        </button>
        <button
          type="button"
          onclick={() => applyPreset('hourly')}
          class="px-2.5 py-1 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded border border-[#2B2E36] text-[11px]"
        >
          Hourly
        </button>
        <button
          type="button"
          onclick={() => applyPreset('daily')}
          class="px-2.5 py-1 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded border border-[#2B2E36] text-[11px]"
        >
          Daily (Midnight)
        </button>
        <button
          type="button"
          onclick={() => applyPreset('workdays')}
          class="px-2.5 py-1 bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-300 rounded border border-[#2B2E36] text-[11px]"
        >
          Weekdays 9 AM
        </button>
      </div>
    </div>

    <div class="grid grid-cols-5 gap-3">
      <div>
        <label for="cron-minute" class="block text-neutral-400 mb-1 font-mono text-[10px] uppercase">Minute (0-59)</label>
        <input
          id="cron-minute"
          type="text"
          bind:value={minute}
          class="w-full bg-[#0E0F12] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 text-center font-mono focus:outline-none focus:border-blue-500"
        />
      </div>
      <div>
        <label for="cron-hour" class="block text-neutral-400 mb-1 font-mono text-[10px] uppercase">Hour (0-23)</label>
        <input
          id="cron-hour"
          type="text"
          bind:value={hour}
          class="w-full bg-[#0E0F12] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 text-center font-mono focus:outline-none focus:border-blue-500"
        />
      </div>
      <div>
        <label for="cron-day" class="block text-neutral-400 mb-1 font-mono text-[10px] uppercase">Day (1-31)</label>
        <input
          id="cron-day"
          type="text"
          bind:value={dayOfMonth}
          class="w-full bg-[#0E0F12] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 text-center font-mono focus:outline-none focus:border-blue-500"
        />
      </div>
      <div>
        <label for="cron-month" class="block text-neutral-400 mb-1 font-mono text-[10px] uppercase">Month (1-12)</label>
        <input
          id="cron-month"
          type="text"
          bind:value={month}
          class="w-full bg-[#0E0F12] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 text-center font-mono focus:outline-none focus:border-blue-500"
        />
      </div>
      <div>
        <label for="cron-dow" class="block text-neutral-400 mb-1 font-mono text-[10px] uppercase">Weekday (0-6)</label>
        <input
          id="cron-dow"
          type="text"
          bind:value={dayOfWeek}
          class="w-full bg-[#0E0F12] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 text-center font-mono focus:outline-none focus:border-blue-500"
        />
      </div>
    </div>

    <div class="p-3 bg-[#0E0F12] rounded-xl border border-[#2B2E36] flex items-center justify-between font-mono text-[11px]">
      <div class="flex items-center gap-3">
        <span class="text-neutral-500 font-sans">Expression:</span>
        <span class="text-blue-400 font-bold">{composedCron}</span>
      </div>
      <div class="text-emerald-400 font-sans font-medium">{cronHumanReadable}</div>
    </div>
  </div>

  <div class="space-y-3">
    <div class="flex items-center justify-between">
      <div>
        <h3 class="text-sm font-semibold text-neutral-200">Scheduled Jobs Pipeline</h3>
        <p class="text-neutral-500 text-[11px]">Active background runners and execution history</p>
      </div>
    </div>

    <div class="border border-[#1A1D24] rounded-xl overflow-hidden bg-[#131416]">
      <table class="w-full text-left border-collapse">
        <thead>
          <tr class="border-b border-[#1A1D24] bg-[#0E0F12] text-neutral-400 text-[10px] uppercase font-semibold">
            <th class="p-3 w-10 text-center">Active</th>
            <th class="p-3">Job Name & Task Prompt</th>
            <th class="p-3">Cron</th>
            <th class="p-3">Next Trigger</th>
            <th class="p-3">Last Run</th>
            <th class="p-3 text-right">Actions</th>
          </tr>
        </thead>
        <tbody class="divide-y divide-[#1A1D24]">
          {#each jobs as job (job.id)}
            <tr class="hover:bg-[#1A1D24]/30 transition-colors">
              <td class="p-3 text-center align-top">
                <input
                  type="checkbox"
                  checked={job.active}
                  onchange={() => toggleJobActive(job)}
                  class="w-4 h-4 rounded border-[#2B2E36] bg-[#0E0F12] text-blue-600 focus:ring-0 cursor-pointer"
                />
              </td>
              <td class="p-3 align-top">
                <div class="font-medium text-neutral-200">{job.name}</div>
                <div class="text-[11px] text-neutral-500 mt-0.5 line-clamp-1">{job.prompt}</div>
              </td>
              <td class="p-3 align-top font-mono text-blue-400 text-[11px]">{job.cronExpr}</td>
              <td class="p-3 align-top font-mono text-neutral-300 text-[11px]">{job.nextRun}</td>
              <td class="p-3 align-top font-mono text-[11px]">
                <div class="text-neutral-300">{job.lastRun || 'Never'}</div>
                {#if job.lastStatus}
                  <span
                    class="px-1.5 py-0.2 rounded text-[9px] uppercase font-bold {job.lastStatus === 'success' ? 'bg-emerald-500/15 text-emerald-400' : job.lastStatus === 'failed' ? 'bg-rose-500/15 text-rose-400' : job.lastStatus === 'running' ? 'bg-blue-500/15 text-blue-400 animate-pulse' : 'text-neutral-500'}"
                  >
                    {job.lastStatus}
                  </span>
                {/if}
              </td>
              <td class="p-3 text-right align-top space-x-2">
                <button
                  type="button"
                  onclick={() => runJobNow(job)}
                  class="px-2.5 py-1 rounded bg-[#1A1D24] hover:bg-[#2B2E36] text-neutral-200 border border-[#2B2E36] transition-colors text-[11px]"
                >
                  Run Now
                </button>
                <button
                  type="button"
                  onclick={() => deleteJob(job.id)}
                  class="text-neutral-500 hover:text-rose-400 px-1.5 py-1 text-[11px]"
                >
                  Delete
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
    </div>
  </div>
</div>

{#if showAddModal}
  <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/80 backdrop-blur-md p-4 text-xs select-none">
    <div class="w-full max-w-lg bg-[#131416] border border-[#2B2E36] rounded-2xl shadow-2xl p-6 space-y-4 text-[#E5E7EB]">
      <h3 class="text-sm font-bold text-neutral-100">Schedule New Task</h3>
      <div class="space-y-3">
        <div>
          <label for="new-job-name" class="block text-neutral-400 mb-1">Task Title</label>
          <input
            id="new-job-name"
            type="text"
            bind:value={newJobName}
            placeholder="e.g. Nightly Git Repo Sync"
            class="w-full bg-[#0E0F12] border border-[#2B2E36] rounded-lg px-3 py-2 text-neutral-200 focus:outline-none focus:border-blue-500"
          />
        </div>
        <div>
          <label for="new-job-prompt" class="block text-neutral-400 mb-1">Agent Instruction Prompt</label>
          <textarea
            id="new-job-prompt"
            bind:value={newJobPrompt}
            placeholder="Provide explicit instructions for the agent to execute when triggered..."
            class="w-full h-24 bg-[#0E0F12] border border-[#2B2E36] rounded-lg p-2.5 text-neutral-200 focus:outline-none focus:border-blue-500 resize-none text-[11px] leading-relaxed"
          ></textarea>
        </div>
        <div class="p-3 bg-[#0E0F12] rounded-lg border border-[#2B2E36]">
          <span class="text-neutral-400 block mb-1">Assigned Schedule</span>
          <span class="font-mono text-blue-400 font-bold">{composedCron}</span>
          <span class="text-neutral-500 text-[11px] ml-2">({cronHumanReadable})</span>
        </div>
      </div>
      <div class="flex justify-end gap-2 pt-2 border-t border-[#1A1D24]">
        <button
          type="button"
          onclick={() => (showAddModal = false)}
          class="px-4 py-2 bg-[#1A1D24] text-neutral-300 rounded-lg hover:bg-[#2B2E36]"
        >
          Cancel
        </button>
        <button
          type="button"
          onclick={handleAddJob}
          class="px-5 py-2 bg-blue-600 text-white rounded-lg hover:bg-blue-500 font-medium"
        >
          Register Schedule
        </button>
      </div>
    </div>
  </div>
{/if}