# Taste
- Prefers the agent to verify functionality empirically by actually exercising the real CLI/commands end-to-end, not just reviewing code or relying on tests alone. Confidence: 0.8
- Wants the agent to autonomously fix/patch errors or missing behavior it encounters rather than only reporting them. Confidence: 0.8
- Comfortable granting the agent privileged (sudo) access and letting it use it as needed to complete the task. Confidence: 0.7
- Prefers eliminating gaps/limitations rather than documenting them as known limitations — wants depth, universality, and completeness across the whole app, and expects the agent to proactively hunt down and patch remaining gaps. Confidence: 0.85
- Likes the agent to parallelize work using subagents (e.g., spawning multiple background audit subagents while implementing the main fix); proactively reaches for "do all the parallel stuff" rather than wanting sequential work. Confidence: 0.85
- Wants outstanding/remaining work written up as a standalone, self-contained markdown file in the repo (not just a chat summary) — structured with categories, priority levels (P1/P2/P3), size estimates, and concrete file/doc references, plus a "completed so far" section for context. Confidence: 0.6
