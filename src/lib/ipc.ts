import { invoke, Channel } from "@tauri-apps/api/core";
import type {
  Provider,
  Model,
  Project,
  Chat,
  Message,
  FileSnapshot,
  MemoryEntry,
  McpServer,
  ScheduledJob,
  ModelUsageStat,
  BootstrapConfig,
  SkillMeta,
  StreamEvent,
  TelegramConfig,
} from "./types";

export function createProject(project: Partial<Project>): Promise<Project> {
  return invoke("create_project", { project });
}

export function listProjects(): Promise<Project[]> {
  return invoke("list_projects");
}

export function getProject(id: string): Promise<Project> {
  return invoke("get_project", { id });
}

export function deleteProject(id: string): Promise<void> {
  return invoke("delete_project", { id });
}

export function createChat(chat: Partial<Chat>): Promise<Chat> {
  return invoke("create_chat", { chat });
}

export function listChats(projectId?: string): Promise<Chat[]> {
  return invoke("list_chats", { projectId });
}

export function getChat(id: string): Promise<Chat> {
  return invoke("get_chat", { id });
}

export function updateChatTitle(id: string, title: string): Promise<void> {
  return invoke("update_chat_title", { id, title });
}

export function updateChatPinned(id: string, isPinned: boolean): Promise<void> {
  return invoke("update_chat_pinned", { id, isPinned });
}

export function deleteChat(id: string): Promise<void> {
  return invoke("delete_chat", { id });
}

export function createMessage(message: Partial<Message>): Promise<Message> {
  return invoke("create_message", { message });
}

export function listMessages(chatId: string): Promise<Message[]> {
  return invoke("list_messages", { chatId });
}

export function deleteMessagesSince(chatId: string, sequenceNum: number): Promise<void> {
  return invoke("delete_messages_since", { chatId, sequenceNum });
}

export function deleteMessage(id: string): Promise<void> {
  return invoke("delete_message", { id });
}

export function recordFileSnapshot(snapshot: FileSnapshot): Promise<void> {
  return invoke("record_file_snapshot", { snapshot });
}

export function getFileSnapshot(snapshotId: string): Promise<FileSnapshot> {
  return invoke("get_file_snapshot", { snapshotId });
}

export function listSnapshotsForChat(chatId: string): Promise<FileSnapshot[]> {
  return invoke("list_snapshots_for_chat", { chatId });
}

export function rollbackFileSnapshot(snapshotId: string): Promise<FileSnapshot> {
  return invoke("rollback_file_snapshot", { snapshotId });
}

export function recordUsageStat(stat: ModelUsageStat): Promise<void> {
  return invoke("record_usage_stat", { stat });
}

export function listUsageStats(providerId?: string, modelId?: string): Promise<ModelUsageStat[]> {
  return invoke("list_usage_stats", { providerId, modelId });
}

export function createProvider(provider: Provider): Promise<void> {
  return invoke("create_provider", { provider });
}

export function listProviders(): Promise<Provider[]> {
  return invoke("list_providers");
}

export function getProvider(id: string): Promise<Provider> {
  return invoke("get_provider", { id });
}

export function updateProvider(provider: Provider): Promise<void> {
  return invoke("update_provider", { provider });
}

export function deleteProvider(id: string): Promise<void> {
  return invoke("delete_provider", { id });
}

export function createModel(model: Model): Promise<void> {
  return invoke("create_model", { model });
}

export function listModels(providerId?: string): Promise<Model[]> {
  return invoke("list_models", { providerId });
}

export function getModel(id: string): Promise<Model> {
  return invoke("get_model", { id });
}

export function updateModel(model: Model): Promise<void> {
  return invoke("update_model", { model });
}

export function deleteModel(id: string): Promise<void> {
  return invoke("delete_model", { id });
}

export function createMcpServer(server: McpServer): Promise<void> {
  return invoke("create_mcp_server", { server });
}

export function listMcpServers(): Promise<McpServer[]> {
  return invoke("list_mcp_servers");
}

export function getMcpServer(id: string): Promise<McpServer> {
  return invoke("get_mcp_server", { id });
}

export function updateMcpServer(server: McpServer): Promise<void> {
  return invoke("update_mcp_server", { server });
}

export function deleteMcpServer(id: string): Promise<void> {
  return invoke("delete_mcp_server", { id });
}

export function createScheduledJob(job: ScheduledJob): Promise<void> {
  return invoke("create_scheduled_job", { job });
}

export function listScheduledJobs(): Promise<ScheduledJob[]> {
  return invoke("list_scheduled_jobs");
}

export function getScheduledJob(id: string): Promise<ScheduledJob> {
  return invoke("get_scheduled_job", { id });
}

export function updateScheduledJob(job: ScheduledJob): Promise<void> {
  return invoke("update_scheduled_job", { job });
}

export function deleteScheduledJob(id: string): Promise<void> {
  return invoke("delete_scheduled_job", { id });
}

export function createMemoryEntry(entry: MemoryEntry): Promise<void> {
  return invoke("create_memory_entry", { entry });
}

export function listMemoryEntries(category?: string): Promise<MemoryEntry[]> {
  return invoke("list_memory_entries", { category });
}

export function getMemoryEntry(id: string): Promise<MemoryEntry> {
  return invoke("get_memory_entry", { id });
}

export function updateMemoryEntry(entry: MemoryEntry): Promise<void> {
  return invoke("update_memory_entry", { entry });
}

export function deleteMemoryEntry(id: string): Promise<void> {
  return invoke("delete_memory_entry", { id });
}

export function searchMemoryEntries(query: string): Promise<MemoryEntry[]> {
  return invoke("search_memory_entries", { query });
}

export function getTotalMemoryTokens(): Promise<number> {
  return invoke("get_total_memory_tokens");
}

export function getBootstrapConfig(): Promise<BootstrapConfig> {
  return invoke("get_bootstrap_config");
}

export function completeFirstRun(workspaceDir?: string, defaultModelId?: string): Promise<void> {
  return invoke("complete_first_run", { workspaceDir, defaultModelId });
}

export function getWorkspaceDir(): Promise<string> {
  return invoke("get_workspace_dir");
}

export function setWorkspaceDir(path: string): Promise<void> {
  return invoke("set_workspace_dir", { path });
}

export function getCredential(keyName: string): Promise<string> {
  return invoke("get_credential", { keyName });
}

export function setCredential(keyName: string, secret: string): Promise<void> {
  return invoke("set_credential", { keyName, secret });
}

export function deleteCredential(keyName: string): Promise<void> {
  return invoke("delete_credential", { keyName });
}

export function startChatStream(
  chatId: string,
  prompt: string,
  modelId: string,
  reasoningEffort: string,
  onEvent: (event: StreamEvent) => void,
): Promise<string> {
  const channel = new Channel<StreamEvent>();
  channel.onmessage = (event) => {
    onEvent(event);
  };
  return invoke<string>("start_chat_stream", {
    chatId,
    prompt,
    modelId,
    reasoningEffort,
    onEvent: channel,
  });
}

export function cancelStream(sessionId: string): Promise<void> {
  return invoke("cancel_stream", { sessionId });
}

export function sendIntervention(sessionId: string, message: string): Promise<void> {
  return invoke("send_intervention", { sessionId, message });
}

export function listSkills(): Promise<SkillMeta[]> {
  return invoke("list_skills");
}

export function importSkillZip(path: string): Promise<void> {
  return invoke("import_skill_zip", { path });
}

export function exportSkillZip(skillName: string, destination: string): Promise<void> {
  return invoke("export_skill_zip", { skillName, destination });
}

export function transcribeAudio(audioData: number[]): Promise<string> {
  return invoke("transcribe_audio", { audioData });
}

export function getTelegramConfig(): Promise<TelegramConfig> {
  return invoke("get_telegram_config");
}

export function setTelegramRunning(run: boolean): Promise<void> {
  return invoke("set_telegram_running", { run });
}
