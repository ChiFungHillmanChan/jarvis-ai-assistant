# 3D Scene Performance Optimization + AI System Prompt Fix

**Date:** 2026-03-25
**Status:** Approved

## Overview

The JarvisScene Canvas2D animation runs ~3,000 draw calls/frame with O(n^2) connection search, causing lag on MacBook Pro. Target: smooth 60fps active, minimal power draw idle. Also fix AI system prompt to mention chart/status rendering capabilities.

## Optimizations

### 1. Reduce particle count: 120 -> 60

In `loadData()`, reduce the target node count from 120 to 60. Real data nodes (~40-50) are kept, filler "particle" nodes reduced to fill remaining slots up to 60. This halves transform, sort, and draw costs.

### 2. Cache z-sort

Add a `sortedNodesRef` that is only recomputed when camera angles change (drag or auto-rotate tick). Track `lastSortRotX`/`lastSortRotY` refs. If rotation hasn't changed since last frame, reuse cached sorted array. During idle (no auto-rotate, no drag), sort runs 0 times/frame.

### 3. Spatial grid for connections

Replace the O(n^2) nested loop for connection drawing with a simple 2D grid:
- Divide canvas into cells of size CONNECTION_DIST (80px)
- Each frame, bucket nodes into grid cells after projection
- For each node, only check neighbors in same + adjacent cells (9 cells max)
- Expected: ~50 data nodes, ~9 checks each = ~450 checks vs 1,225

### 4. Batch Canvas2D paths

- **Grid lines**: Combine all latitude lines into one `beginPath()...stroke()` call, same for longitude. Reduces 27 strokes to 2.
- **Node outlines**: Batch non-hovered node circles into a single path.
- **Connections**: Already batched (single path). Keep as-is.

### 5. Adaptive frame throttling

- Idle: **10fps** (100ms interval) -- scene barely moves
- Active: **60fps** (16ms interval) -- voice, drag, transitions
- Add 2-second cooldown after last interaction before dropping to idle
- Fix rAF double-queuing: only call `requestAnimationFrame` once per frame cycle

### 6. Skip grid lines when idle

Latitude/longitude grid is ~700 lineTo calls. When idle:
- Fade grid opacity to 0 over 500ms
- Skip grid drawing entirely when opacity < 0.01
- Fade back in on any interaction or activity change

### 7. AI system prompt update

In `src-tauri/src/ai/tools.rs`, update the system prompt's capabilities list to include:
```
data visualization (inline charts and status cards)
```
Update the tool count from 32 to 34.

## Files

**Modified:**
- `src/components/3d/JarvisScene.tsx` -- all Canvas2D optimizations
- `src-tauri/src/ai/tools.rs` -- system prompt update

## Expected Impact

| Metric | Before | After |
|--------|--------|-------|
| Draw calls/frame (active) | ~3,000 | ~1,000 |
| Draw calls/frame (idle) | ~3,000 | ~300 |
| Connection checks/frame | 1,225 | ~450 |
| Sort operations/frame (idle) | 1 | 0 |
| Node count | 120 | 60 |
| Idle FPS | 20 | 10 |
| Active FPS | 60 | 60 |
