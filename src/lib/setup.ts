import { invoke } from '@tauri-apps/api/core';
import type { MailSettings } from './mail';

export interface CapabilityStatus {
  scope: 'mail';
  state: 'not_configured' | 'saved_unverified' | 'ready' | 'action_required';
  provider: string | null;
  model: string | null;
  checkedAt: string | null;
  errorCode: string | null;
  credentialAvailable: boolean;
}
export interface SetupStatus {
  mail: CapabilityStatus;
  gmail: { connectedAccounts: number; totalAccounts: number; messageCount: number };
}
export interface LocalSetupProbe {
  os: string; arch: string; macosVersion: string | null; totalRamGb: number;
  availableDiskGb: number | null; ollamaInstalled: boolean; ollamaRunning: boolean;
  recommendedTag: string | null; downloadUrl: string; installedModels: string[];
}
export interface LocalSetupProgress {
  jobId: string; stage: 'checking' | 'starting' | 'server_ready' | 'needs_install' | 'failed' | 'cancelled';
  model: string; completedBytes: number | null; totalBytes: number | null; errorCode: string | null;
}
export const getSetupStatus = () => invoke<SetupStatus>('get_setup_status');
export const testMailAiConnection = (input?: MailSettings) => invoke<CapabilityStatus>('test_mail_ai_connection', { input });
export const applyTestedMailSettings = (input: MailSettings, expected: MailSettings) => invoke<void>('apply_tested_mail_settings', { input, expected });
export const probeLocalSetup = () => invoke<LocalSetupProbe>('probe_local_setup');
export const prepareLocalAi = (model: string, jobId: string) => invoke<string>('prepare_local_ai', { scope: 'mail', model, jobId });
export const cancelLocalSetup = (jobId: string) => invoke<void>('cancel_local_setup', { jobId });

export function setupError(code: string | null, text: (en: string, zh: string) => string): string {
  switch (code) {
    case 'settings_changed': return text('Mail settings changed in another view. Reload this page before applying the model.', '電郵設定已在另一個畫面變更。請重新載入後再套用模型。');
    case 'probe_required': return text('Settings or credentials changed after testing. Test this selection again.', '設定或憑證已在測試後變更，請重新測試目前的選項。');
    case 'model_query_failed': return text('Could not check installed Ollama models. Open Ollama and retry; nothing will be downloaded until the check succeeds.', '未能核對已安裝的 Ollama 模型。請開啟 Ollama 後重試；核對成功前不會下載模型。');
    case 'cancelled': return text('Cancelled. Downloaded files are kept for a retry.', '已取消；已下載的檔案會保留，以便重試。');
    case 'ollama_not_installed': return text('Install Ollama from its official site, then return and retry.', '請先從官方網站安裝 Ollama，然後返回此頁重試。');
    case 'insufficient_disk_or_unknown': return text('There is not enough free disk space, or it could not be checked. Free some space and retry.', '磁碟空間不足或未能完成檢查。請騰出空間後重試。');
    case 'insufficient_ram': return text('This model needs more memory. Choose a smaller model.', '此模型需要更多記憶體，請選擇較小的模型。');
    case 'unsupported_os': case 'unsupported_os_version': return text('This setup requires macOS 14 or newer.', '此設定流程需要 macOS 14 或更新版本。');
    case 'setup_busy': case 'pull_busy': return text('Another setup or download is running. Wait for it to finish.', '另一項設定或下載仍在進行中，請等待完成。');
    case 'credential_unavailable': return text('Saved credentials could not be read. Check Keychain access and retry; do not replace the key just because of this error.', '未能讀取已儲存的憑證。請檢查鑰匙圈存取權限後重試；無須因此重新申請密鑰。');
    case 'credential_missing': case 'missing_credentials': case 'missing_key': return text('Save the credentials for the selected provider, then test again.', '請先儲存所選供應商的憑證，再進行測試。');
    case 'incorrect_facts': case 'synthetic_check_failed': return text('The model did not pass the sample check. Try another model.', '模型未通過示範資料核對，請嘗試其他模型。');
    case 'server_not_ready': return text('Ollama did not start in time. Open Ollama and retry.', 'Ollama 未能及時啟動。請開啟 Ollama 後重試。');
    default: return text('The test or setup did not finish. Check the selected model, credentials, balance and connection, then retry. Your mail settings have not been switched.', '測試或設定未完成。請檢查模型、憑證、餘額及連線後重試；電郵處理方式尚未切換。');
  }
}
