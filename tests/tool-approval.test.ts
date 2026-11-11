import test from 'node:test';
import assert from 'node:assert/strict';
import { describeApproval } from '../src/lib/tool-approval.ts';

// The approval card is the only thing between a crafted email and the action
// it asks for, so the sentence on it has to say what will happen and to what.

const en = (english: string) => english;
const zh = (_english: string, chinese: string) => chinese;
const request = (tool_name: string, detail: string) => ({ approval_id: 'a1', tool_name, detail, status: null });

test('opening a file names the path in both languages', () => {
  assert.equal(describeApproval(request('open_file', '~/Downloads/run.command'), en), 'Open the file ~/Downloads/run.command');
  assert.equal(describeApproval(request('open_file', '~/Downloads/run.command'), zh), '開啟檔案 ~/Downloads/run.command');
});

test('opening an app names the app in both languages', () => {
  assert.equal(describeApproval(request('open_app', 'Terminal'), en), 'Open the app Terminal');
  assert.equal(describeApproval(request('open_app', 'Terminal'), zh), '開啟應用程式 Terminal');
});

test('a follow-up change says which change', () => {
  assert.equal(describeApproval({ ...request('update_follow_up', '“Pay Kenny”'), status: 'done' }, en), 'Mark the follow-up “Pay Kenny” as done');
});
