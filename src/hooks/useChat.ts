import { useState, useEffect, useCallback, useRef } from "react";
import { listen } from "@tauri-apps/api/event";
import type { ChatMessage, AiState, ChatTokenPayload, ChatStatusPayload, ChatStatePayload } from "../lib/types";
import { sendMessage, getConversations } from "../lib/commands";
import { invoke } from "@tauri-apps/api/core";

export function useChat() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [aiState, setAiState] = useState<AiState>("idle");
  const [currentStatus, setCurrentStatus] = useState<string | null>(null);
  const [streamingText, setStreamingText] = useState("");

  // Token buffer for requestAnimationFrame batching
  const tokenBuffer = useRef("");
  const rafId = useRef<number | null>(null);
  const streamingTextRef = useRef("");
  // Track whether voice initiated the current AI request -- skip streaming tokens if so
  const voiceActiveRef = useRef(false);
  // Track whether chat (useChat.send) initiated the current request
  const chatInitiatedRef = useRef(false);

  useEffect(() => { getConversations().then(setMessages).catch((e) => setError(String(e))); }, []);

  // Event listeners for streaming
  useEffect(() => {
    let mounted = true;

    const flushBuffer = () => {
      if (!mounted) return;
      if (tokenBuffer.current) {
        const newText = tokenBuffer.current;
        tokenBuffer.current = "";
        setStreamingText((prev) => {
          const updated = prev + newText;
          streamingTextRef.current = updated;
          return updated;
        });
      }
      rafId.current = null;
    };

    // Track voice-initiated requests to skip token processing
    const unlistenVoice = listen<{ active: boolean }>("chat-voice-active", (event) => {
      voiceActiveRef.current = event.payload.active;
      // When voice finishes, clear any leftover streaming state
      if (!event.payload.active) {
        tokenBuffer.current = "";
        if (rafId.current) {
          cancelAnimationFrame(rafId.current);
          rafId.current = null;
        }
      }
    });

    const unlistenToken = listen<ChatTokenPayload>("chat-token", (event) => {
      if (!mounted) return;
      // Skip token processing if voice initiated this request (not our concern)
      if (voiceActiveRef.current && !chatInitiatedRef.current) return;

      if (event.payload.done) {
        if (tokenBuffer.current) {
          const remaining = tokenBuffer.current;
          tokenBuffer.current = "";
          setStreamingText((prev) => {
            const updated = prev + remaining;
            streamingTextRef.current = updated;
            return updated;
          });
        }
        if (rafId.current) {
          cancelAnimationFrame(rafId.current);
          rafId.current = null;
        }
        return;
      }
      tokenBuffer.current += event.payload.token;
      if (!rafId.current) {
        rafId.current = requestAnimationFrame(flushBuffer);
      }
    });

    const unlistenStatus = listen<ChatStatusPayload>("chat-status", (event) => {
      // Only update status for chat-initiated requests
      if (mounted && !voiceActiveRef.current) setCurrentStatus(event.payload.status);
    });

    const unlistenState = listen<ChatStatePayload>("chat-state", (event) => {
      if (mounted) setAiState(event.payload.state);
    });

    // Listen for new messages from voice or other non-chat paths
    const unlistenNewMsg = listen<{ role: string; content: string }>("chat-new-message", (event) => {
      if (!mounted) return;
      const msg: ChatMessage = {
        id: null,
        role: event.payload.role as "user" | "assistant",
        content: event.payload.content,
        created_at: new Date().toISOString(),
      };
      setMessages((prev) => [...prev, msg]);
    });

    return () => {
      mounted = false;
      if (rafId.current) cancelAnimationFrame(rafId.current);
      unlistenToken.then((fn) => fn());
      unlistenStatus.then((fn) => fn());
      unlistenState.then((fn) => fn());
      unlistenNewMsg.then((fn) => fn());
      unlistenVoice.then((fn) => fn());
    };
  }, []);

  async function send(text: string) {
    if (!text.trim() || loading) return;
    const userMessage: ChatMessage = { id: null, role: "user", content: text, created_at: new Date().toISOString() };
    setMessages((prev) => [...prev, userMessage]);
    setLoading(true);
    setError(null);
    setStreamingText("");
    setCurrentStatus(null);
    tokenBuffer.current = "";
    streamingTextRef.current = "";
    chatInitiatedRef.current = true;

    try {
      const response = await sendMessage(text);
      const finalContent = streamingTextRef.current || response.content;
      setMessages((prev) => [...prev, { ...response, content: finalContent }]);
      setStreamingText("");
      streamingTextRef.current = "";
    } catch (e) {
      setError(String(e));
      if (streamingTextRef.current) {
        setStreamingText((prev) => prev + " [incomplete]");
      }
    } finally {
      setLoading(false);
      chatInitiatedRef.current = false;
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

  return { messages, loading, error, send, clearChat, aiState, currentStatus, streamingText };
}
