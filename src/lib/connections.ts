import { invoke } from '@tauri-apps/api/core';

export type IntegrationService = 'github' | 'notion' | 'obsidian';
export interface ConnectionStatus {
  status: 'not_connected' | 'configured' | 'connected';
  identity: string | null;
  method: string | null;
  message: string | null;
  oauth_ready?: boolean;
}
export interface GitHubDevicePrompt {
  flow_id: string;
  user_code: string;
  verification_uri: string;
  expires_in: number;
}
export const connectionStatus = (service: IntegrationService): Promise<ConnectionStatus> => invoke(`${service}_connection_status`);
export const disconnectService = (service: IntegrationService): Promise<void> => invoke(`${service}_disconnect`);
export const githubBeginConnect = (): Promise<GitHubDevicePrompt> => invoke('github_begin_connect');
export const githubCompleteConnect = (flowId: string): Promise<ConnectionStatus> => invoke('github_complete_connect', { flowId });
export const cancelConnect = (service: 'github' | 'notion'): Promise<void> => invoke(`${service}_cancel_connect`);
export const notionConnect = (): Promise<ConnectionStatus> => invoke('notion_connect');
export const obsidianSelectVault = (): Promise<ConnectionStatus> => invoke('obsidian_select_vault');
export const openObsidian = (): Promise<void> => invoke('open_obsidian');
export const githubSetClientId = (clientId: string): Promise<void> => invoke('github_set_client_id', { clientId });
export function connectionsChanged() { window.dispatchEvent(new Event('jarvis:connections-changed')); }
