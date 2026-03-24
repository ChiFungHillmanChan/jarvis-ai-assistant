# JARVIS UI/UX Animation & Conversation Persistence Design

**Date:** 2026-03-24
**Status:** Approved

## Overview

Four improvements to the JARVIS desktop assistant that bring it closer to the Marvel JARVIS experience: conversation persistence across restarts, streaming sentence-by-sentence TTS, a radial waveform visualization during speech, and energy arc animations for tool calls.

## 1. Conversation Persistence Across App Restarts

### Problem
When the user closes and reopens the app, the chat panel shows no previous messages despite them being stored in the SQLite `conversations` table.

### Solution
Ensure `useChat.ts` loads and displays saved messages on mount.

### Changes

**`src/hooks/useChat.ts`:**
- On mount, call `getConversations()` to fetch messages from DB
- Populate the `messages` state array with the loaded history
- Messages render immediately when chat panel opens

**`src/components/ChatPanel.tsx`:**
- After loading history, scroll message list to bottom
- No visual change -- messages display the same as during a live conversation

**`src-tauri/src/commands/chat.rs`:**
- Verify `getConversations` returns messages in chronological order (it queries with `ORDER BY id ASC`, which is monotonically chronological)

**Scope exclusions:**
- No conversation list/sidebar (just loads the most recent conversation)
- The "NEW" button still clears local state for a fresh conversation

## 2. Streaming Sentence-by-Sentence TTS

### Problem
Current TTS waits for the full AI response before speaking. This creates a long pause that breaks the conversational feel.

### Solution
Buffer streamed tokens, detect sentence boundaries, and dispatch each sentence to TTS immediately. Sentences queue and play sequentially.

### Changes

**`src-tauri/src/commands/chat.rs`:**
- Add a `SentenceBuffer` struct that accumulates tokens from the AI stream
- Detect sentence boundaries: `. ` `! ` `? ` or `\n` after >= 20 characters
- When a sentence boundary is detected, send the completed sentence to a TTS queue channel
- Spawn a tokio task that reads from the TTS queue and plays sentences sequentially
- Remove the existing `chat-state: speaking` emission (currently at line ~232) -- this will now be driven by TTS events instead
- Remove the existing `chat-state: idle` emission after TTS (currently at line ~240) -- replaced by TTS queue completion

**`src-tauri/src/voice/tts.rs`:**
- Add `speak_queued(sentence: String, app_handle: AppHandle)` method
- Uses a `tokio::sync::mpsc` channel to queue sentences
- Plays sentences sequentially (each `say` process completes before next starts)
- Emits `tts-speaking` event when each sentence starts. Payload: `{ sentence: string, remaining: number }`
- Emits `tts-sentence-done` event when each sentence finishes. Payload: `{ remaining: number }`. When `remaining === 0`, all speech is complete.
- Emits `tts-amplitude` events during playback at ~15Hz. Payload: `{ amplitude: number }` (0.0 to 1.0). Estimated from text syllable density with sine-wave simulation (real audio FFT is a non-goal).
- Emits `chat-state: speaking` when the first sentence begins TTS playback
- Emits `chat-state: idle` when the last sentence finishes (remaining === 0)

**`src/hooks/useChat.ts`:**
- No changes needed for `aiState` -- it already listens for `chat-state` events, and `tts.rs` now emits those
- Listen for `tts-amplitude` events and store the latest amplitude in a ref (passed to JarvisScene for waveform rendering)
- Listen for `chat-tool-call` events and store in a ref/state (passed to JarvisScene for energy arc creation)

**`src/components/3d/JarvisScene.tsx`:**
- Receives `ttsAmplitude` and `toolCalls` as props from the parent
- Uses `ttsAmplitude` to drive radial waveform bar heights
- Uses `toolCalls` to spawn energy arcs

### Sentence Boundary Rules
- Primary delimiters: `.` `!` `?` followed by a space or end-of-stream
- Minimum sentence length: 20 characters (prevents splitting on abbreviations like "Dr. Smith")
- Maximum buffer size: 200 characters -- force flush at nearest word boundary to prevent extremely long sentences from delaying TTS
- End-of-stream: flush remaining buffer as final sentence regardless of delimiter (skip if whitespace-only)
- Skip empty/whitespace-only sentences
- Known edge cases (acceptable for v1): ellipsis (`...`) may trigger early split, decimal numbers like `98.6` may split, URLs with dots may split, bulleted lists with `\n- ` may produce short fragments. These are cosmetic TTS issues, not functional bugs -- refine in a follow-up iteration if needed

### Latency Target
First sentence should begin speaking within ~0.5-1.5 seconds of the AI starting generation (depends on AI token speed for first sentence completion).

## 3. Radial Waveform Visualization During Speech

### Problem
When JARVIS speaks, there is no visual feedback in the atom core showing audio activity. The experience lacks the cinematic feel of Marvel's JARVIS.

### Solution
Add a circular equalizer (radial waveform) to the atom core that activates during TTS playback, crossfading with the existing icosahedron wireframe.

### Integration Point
All rendering happens inside `JarvisScene.tsx` Canvas2D render loop (Approach A -- single render pipeline).

### Visual Specification

**Radial waveform geometry:**
- 48 bars radiating outward from center
- Inner radius: 18px (bars start here)
- Max outer radius: 55px (bars extend to here at full amplitude)
- Each bar: 2.5px stroke width
- Bar color: `rgba(0, 180, 255, 0.4 + amplitude * 0.5)` -- brighter at higher amplitude
- Bright tip dot: 2px radius circle at each bar's outer end
- Inner ring: 1px circle at 18px radius, `rgba(0, 180, 255, 0.3)`
- Center dot: 3px radius, `rgba(0, 180, 255, 0.9)`

**Amplitude data:**
- Backend emits `tts-amplitude` events during TTS playback
- Each event contains a normalized amplitude value (0.0 to 1.0)
- If real audio analysis is too costly, estimate from text rhythm: use a sine-wave simulation with frequency based on syllable density
- Frontend stores latest amplitude in a ref, waveform reads it each frame
- Each bar's amplitude is offset by its angle to create a rotating wave pattern, not uniform pulsing

**State-driven crossfade:**
- New `speakingAlpha` float (0.0 to 1.0) in the render state
- When TTS starts (`tts-speaking` event): interpolate `speakingAlpha` from 0 -> 1 over 300ms
- When TTS ends (all sentences done): interpolate `speakingAlpha` from 1 -> 0 over 500ms
- Icosahedron opacity: `1 - speakingAlpha * 0.7` (fades to 30% during speech)
- Waveform opacity: `speakingAlpha` (fades in during speech)
- Core glow radius increases slightly during speech (50px -> 60px)

**New function:** `drawRadialWaveform(ctx, cx, cy, amplitude, speakingAlpha, time)`

## 4. Tool Call Energy Arc Animation

### Problem
When the AI uses function calls (checking calendar, searching emails, etc.), there is no visual indication that JARVIS is "dispatching work." The atom looks the same during thinking and tool execution.

### Solution
Two layered animations: orbital rings accelerate during thinking, and energy arcs fire from the atom core to matching data nodes when tool calls execute.

### Visual Specification

**Orbital ring acceleration:**
- New `ringSpeedMultiplier` float in render state, interpolated via lerp
- Target values by activity level (matches existing `ActivityLevel` type): `idle`=1.0, `listening`=1.2, `processing`=3.0, `active`=1.0
- Acceleration lerp: 500ms to reach target
- Deceleration lerp: 800ms to reach target
- When `ringSpeedMultiplier > 1.5`: orbital dots get trailing afterimage (4 fading dots behind, each 0.22 less alpha)

**Energy arc system:**

*Data structure:*
```typescript
interface EnergyArc {
  target: DataNode;        // destination node on the sphere
  progress: number;        // 0.0 to 1.0
  speed: number;           // 0.025 to 0.04 (randomized for variety)
  trail: { x: number, y: number }[];  // last 15 positions
  color: { r: number, g: number, b: number };  // matches target type
  active: boolean;
}
```

*Arc path:*
- Quadratic bezier curve from atom center to target node position
- Control point offset perpendicular to the direct line (alternating left/right for visual variety)
- Easing: ease-out cubic (`1 - (1 - t)^3`) -- fast launch, decelerating arrival
- Total travel time: ~1.5 seconds (variable via speed parameter)

*Arc rendering:*
- Glowing head: 8px radial gradient, full type color at center fading to transparent
- Fading trail: last 15 positions, connected as line segments with increasing alpha and width
- Trail width: grows from 0.5px to 2.5px toward head
- Trail alpha: grows from 0 to 0.7 toward head

*On arrival:*
- Target node flash: `flashAlpha` set to 1.0, decays via `*= 0.96` per frame
- Target node scale: `flashScale` set to 1.8, decays back to 1.0 via `+= (1 - scale) * 0.08` per frame
- Glow ring appears around target node during flash

*Tool type to node type mapping:*
| Tool name pattern | Node type | Color |
|---|---|---|
| `*calendar*`, `*event*` | meeting | amber (255, 180, 0) |
| `*email*`, `*gmail*` | email | light cyan (100, 200, 255) |
| `*github*`, `*pr*`, `*issue*` | github | green (16, 185, 129) |
| `*notion*` | notion | purple (180, 130, 255) |
| `*cron*`, `*schedule*` | cron | teal (0, 220, 200) |
| `*note*`, `*obsidian*` | notion | purple (180, 130, 255) |
| `*task*`, all other tools (default) | task | cyan (0, 180, 255) |

Note: ~16 system tools (open_app, run_command, clipboard, screenshot, etc.) all map to the default task/cyan. This is acceptable -- system tools don't have a distinct node type in the data sphere. If visual variety feels insufficient in practice, a "system" node type could be added in a follow-up.

**Backend event:**
- `chat-tool-call` event emitted from `ai/claude.rs` and `ai/openai.rs` (where tool invocations are dispatched), NOT from `commands/chat.rs` which has no visibility into individual tool calls
- Payload: `{ tool_name: string }` (frontend maps to node type using the table above)
- Emitted before the tool executes so the arc animation plays during execution
- Both AI providers already emit `chat-status` events from these locations -- `chat-tool-call` follows the same pattern

**New functions:**
- `createEnergyArc(toolName: string, nodes: DataNode[])` -- creates arc targeting nearest node of matching type
- `drawEnergyArc(ctx, arc, time)` -- renders single arc with trail and head
- `updateArcs(arcs)` -- advances all active arcs and cleans up completed ones

## Full Voice-to-Response Animation Sequence

1. **User speaks** -- VoiceIndicator shows "LISTENING", atom activity = `listening`, rings at 1.2x
2. **User stops** -- Audio captured, atom activity = `processing`, rings accelerate to 3x
3. **AI thinks** -- Radial waveform dormant, rings spinning fast, icosahedron pulsing at processing speed
4. **Tool calls fire** -- Energy arcs shoot from core to matching nodes (one per tool call), nodes flash on arrival
5. **First sentence ready** -- TTS starts, `speakingAlpha` crossfades in over 300ms, radial waveform activates, rings decelerate to 1x
6. **Streaming continues** -- TTS speaks sentence-by-sentence, waveform amplitude follows speech rhythm, new arcs can still fire if AI calls more tools mid-response
7. **Last sentence done** -- Waveform fades out over 500ms, icosahedron fades back in, atom returns to idle

## File Change Summary

| File | Change Type | Description |
|---|---|---|
| `src/hooks/useChat.ts` | Modify | Load messages on mount, listen for TTS events |
| `src/components/ChatPanel.tsx` | Modify | Scroll to bottom after loading history |
| `src/components/3d/JarvisScene.tsx` | Modify | Add waveform drawing, energy arcs, ring speed multiplier, speakingAlpha crossfade |
| `src-tauri/src/commands/chat.rs` | Modify | Sentence buffer, remove old chat-state speaking/idle emissions, add TTS cancellation on new message |
| `src-tauri/src/ai/claude.rs` | Modify | Emit `chat-tool-call` event before tool execution |
| `src-tauri/src/ai/openai.rs` | Modify | Emit `chat-tool-call` event before tool execution |
| `src-tauri/src/voice/tts.rs` | Modify | Sentence queue, sequential playback, amplitude events |
| `src-tauri/src/voice/commands.rs` | Modify | Remove separate AI+TTS calls, remove chat-state speaking/idle emissions, remove VoiceState::Speaking transition, route transcribed text through send_message |
| `src/hooks/useVoiceState.ts` | No change | VoiceState transitions continue to work via existing `voice-state` event listener |
| `src/lib/types.ts` | Minor | Add ToolCallEvent type |

## Cancellation Behavior

When the user sends a new message while TTS is still playing from a previous response:
1. Cancel any in-progress TTS playback (`tts.rs` kills the active `say` process)
2. Clear the TTS sentence queue
3. Flush and reset the sentence buffer
4. Emit `chat-state: idle` immediately to reset the UI state
5. Then proceed with the new message normally

This prevents interleaving of old and new response audio.

## Voice Pipeline Scope

The voice pipeline in `voice/commands.rs` (the `stop_listening` -> transcribe -> AI -> TTS path) also calls `tts.speak()` after getting a full AI response. For v1, the voice pipeline will use the same new sentence-queued TTS path: `commands/chat.rs` already handles the voice-originated messages via `chat-new-message` events, so the sentence buffer and TTS queue apply to both text and voice inputs.

**Specific changes to `voice/commands.rs`:**
- Remove the `tts.speak(&response)` call and surrounding `chat-state: speaking` / `chat-state: idle` emissions (currently at lines ~126-138)
- Keep the `chat-state: thinking` emission at line ~91 (this is correct -- it signals AI processing started)
- Keep `VoiceState::Processing` transition (signals audio is being transcribed)
- Remove `VoiceState::Speaking` transition at line ~127 -- `tts.rs` will handle this via the unified path
- After transcription, route the transcribed text through `send_message` (the unified chat path) instead of calling the AI router and TTS directly. This ensures sentence buffering, tool call events, and TTS queuing all apply consistently regardless of input method.

## Performance Constraints

- Waveform rendering: 48 bars per frame is negligible in Canvas2D
- Energy arcs: max ~5 concurrent arcs expected, each is a few draw calls
- Ring speed interpolation: single lerp per frame
- TTS amplitude events: throttled to ~15Hz to avoid overwhelming the event bridge
- Overall: the Canvas2D render loop runs continuously via requestAnimationFrame. The new waveform and arc code adds conditional branches (gated on `speakingAlpha > 0` and `arcs.length > 0`) that skip draw calls when inactive. Incremental CPU cost during active animation is negligible; idle cost is one branch check per frame.

## Non-Goals

- No conversation list/sidebar (just persistence of last conversation)
- No real audio FFT analysis (simulated amplitude is sufficient for visual effect)
- No WebGL migration (Canvas2D approach is sufficient and performant)
- No changes to the voice capture or wake word systems
