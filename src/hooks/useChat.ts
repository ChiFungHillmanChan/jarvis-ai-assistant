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
  // Text from a provider that failed mid-stream, held aside rather than destroyed:
  // the router only predicts that another provider will RUN, not that it will
  // produce anything, so on an outage that takes out the whole chain the discarded
  // text is all the user ever had. Cleared when a replacement token actually
  // arrives; restored by `send`'s catch if the chain fails outright.
  const discardedTextRef = useRef("");
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
        // A provider failed mid-stream AND the router is about to run another one:
        // clear the partial output so the next provider's answer is not appended to
        // half an answer from this one. The router only sets `discard` in that case
        // -- on a terminal failure the partial answer is all the user gets, so it
        // stays on screen and `send`'s catch marks it " [incomplete]".
        if (event.payload.discard) {
          // Held aside, not thrown away: "another provider will run" is not
          // "another provider will produce tokens". On an outage that kills every
          // remaining entry, nothing replaces this text, and deleting it here is
          // what left the pane blank where the user previously had a partial
          // answer. It is dropped for real on the replacement's first token.
          const pending = tokenBuffer.current;
          tokenBuffer.current = "";
          streamingTextRef.current = "";
          // Reset through the updater queue as well: a flush queued before this event
          // would otherwise write the failed provider's text back into the ref. `prev`
          // there is also the authoritative partial answer -- it includes any flush the
          // ref has not caught up with yet -- so that is what gets held aside.
          setStreamingText((prev) => {
            discardedTextRef.current = prev + pending;
            streamingTextRef.current = "";
            return "";
          });
          if (rafId.current) {
            cancelAnimationFrame(rafId.current);
            rafId.current = null;
          }
          return;
        }
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
      // The replacement has started arriving, so the discarded text is genuinely
      // superseded and can go.
      if (discardedTextRef.current) discardedTextRef.current = "";
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
      if (!mounted) return;
      setAiState(event.payload.state);
      if (event.payload.state === "idle") {
        setCurrentStatus(null);
      }
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

  async function send(text: string, workload?: "tools" | "premium") {
    if (!text.trim() || loading) return;
    const userMessage: ChatMessage = { id: null, role: "user", content: text, created_at: new Date().toISOString() };
    setMessages((prev) => [...prev, userMessage]);
    setLoading(true);
    setError(null);
    setStreamingText("");
    setCurrentStatus(null);
    tokenBuffer.current = "";
    streamingTextRef.current = "";
    discardedTextRef.current = "";
    chatInitiatedRef.current = true;

    try {
      const response = await sendMessage(text, workload);
      const finalContent = streamingTextRef.current || response.content;
      setMessages((prev) => [...prev, { ...response, content: finalContent }]);
      setStreamingText("");
      streamingTextRef.current = "";
      discardedTextRef.current = "";
    } catch (e) {
      setError(String(e));
      // The whole chain failed. If a provider streamed a partial answer that was
      // discarded for a replacement that never produced anything, give it back --
      // a partial answer plus an error beats a blank pane plus an error. Captured
      // into a local first: the updater runs after this handler, by which point
      // the ref has been cleared.
      const held = discardedTextRef.current;
      discardedTextRef.current = "";
      if (streamingTextRef.current || held) {
        setStreamingText((prev) => (prev || held) + " [incomplete]");
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
