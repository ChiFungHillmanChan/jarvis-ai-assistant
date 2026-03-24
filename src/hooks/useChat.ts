import { useState, useEffect, useCallback } from "react";
import type { ChatMessage } from "../lib/types";
import { sendMessage, getConversations } from "../lib/commands";
import { invoke } from "@tauri-apps/api/core";

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => { getConversations().then(setMessages).catch((e) => setError(String(e))); }, []);

  async function send(text: string) {
    if (!text.trim() || loading) return;
    const userMessage: ChatMessage = { id: null, role: "user", content: text, created_at: new Date().toISOString() };
    setMessages((prev) => [...prev, userMessage]);
    setLoading(true);
    setError(null);
    try {
      const response = await sendMessage(text);
      setMessages((prev) => [...prev, response]);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }

  const clearChat = useCallback(async () => {
    try {
      await invoke("clear_conversations");
      setMessages([]);
      setError(null);
    } catch (e) {
      console.error("Failed to clear chat:", e);
    }
  }, []);

  return { messages, loading, error, send, clearChat };
}
