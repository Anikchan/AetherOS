# F10 — Optional Basic AI Assistant

Owner: `nexa-ai` (AI runtime service and assistant UI). Internal design: D8 section 6.

## 1. Goal

A very basic, local language model that is available only if the user chooses to install it. It helps with simple text tasks. It is off by default, sends nothing to the internet, and costs nothing (no memory, no CPU, no disk) while it is not in use.

Realistic expectation: small local models are useful for short tasks (summaries, rewrites, short answers) but make mistakes and are slow on old CPUs. They are not a replacement for large cloud assistants.

## 2. Principles

1. **Optional.** Not in the base image. Installed from Settings or during first-run setup only if the user agrees.
2. **Local only.** No data leaves the machine. No telemetry. No accounts.
3. **Removable.** One click removes the model and its files completely.
4. **Managed like an app.** The AI runtime is managed by the process manager (F2): it is suspended and unloaded when idle, and it respects battery rules (F7).
5. **Small.** Basic models only in v1.

## 3. Feature List

| ID | Feature | Priority | Description |
| --- | --- | --- | --- |
| AI-01 | Opt-in install | MVP (Later for the model download itself) | Settings page offers "Install assistant" with size, RAM need, and what it does. Off by default. |
| AI-02 | Model package | MVP | One signed model file in a quantized format. Size up to 4 to 8 GB total, counted with the OS size, depending on the model chosen (owner question Q14). |
| AI-03 | Assistant in Text | v1 | Actions on selected text: summarize, rewrite shorter, fix grammar. Results appear as a suggestion; the user accepts or discards. |
| AI-04 | Ask bar in launcher | v1 | Type a question after a prefix character; the answer shows in the launcher panel. One turn only in v1. |
| AI-05 | Load on demand | MVP | Model loads when the user first asks, and not at boot. |
| AI-06 | Unload when idle | MVP | Unloads after 2 minutes idle (configurable). Its memory is released. |
| AI-07 | Battery rule | MVP | On battery, the assistant asks before it runs a long answer. Can be turned off in Settings (F7 policy). |
| AI-08 | Hardware gate | MVP | Disabled with a message on machines with less than 4 GB RAM or without AVX2 support. |
| AI-09 | Stop and limits | MVP | Stop button during answers. Maximum answer length and time limit set in config (F4). |
| AI-10 | Remove | MVP | Removes the model, cache, and history. Nothing remains. |
| AI-11 | Local history | v1 | Optional; off by default. Stored only on disk in the user's home folder. |
| AI-12 | No system control | Policy | The assistant cannot change settings, run commands, or open files on its own in v1. |

## 4. Resource Expectations

| Measure | Expectation (to be measured in P7) |
| --- | --- |
| Disk size of the runtime | About 10 to 20 MB (runtime library) |
| Disk size of the model | Up to 4 to 8 GB total with the OS size for the chosen model (owner Q14) |
| RAM when loaded | About 0.6 to 1.5 GB, depending on model size and answer length |
| RAM when unloaded | 0 MB |
| CPU during an answer | Short bursts on all cores; a few tokens per second on older CPUs |
| Idle CPU | 0% |
| Battery | Each long answer costs battery; the battery rule (AI-07) applies |

These are planning estimates. Real figures are recorded in the D6 benchmark results.

## 5. Model and Runtime Selection

- Runtime: an open-source inference library (llama.cpp, MIT license), included as an internal engine. The user never sees it as an app.
- Model: Qwen3.5 4B (4-bit quantized), chosen by the owner on 2026-11-17, as the default. Owner decision Q23 (2026-11-17): use a lighter default model for 4 GB machines, but keep the option for users to install larger models (up to 8 GB) when their hardware allows. License check and a short quality test on the owner's text tasks still happen in P7 before it ships. Each model's license is recorded before use.
- Model format: quantized (4-bit or similar) so it fits in memory.
- Model updates: signed, delivered like other OS components.

Owner question Q14 was answered on 2026-11-17: target up to 4 to 8 GB total with the OS size; model is Qwen3.5 4B (4-bit quantized).

## 6. Dependencies

- P7-AI phase (optional, after Personal Edition core M5).
- F2 process manager (AI runtime lifecycle).
- F7 battery rules.
- F8 Text app (AI-03).
- F9 app platform (install and remove of the model package).

## 7. Acceptance Criteria

| ID | Criterion | Result |
| --- | --- | --- |
| AI-AC-01 | With the assistant not installed, there are no AI processes, no AI files, and no AI memory use | Not tested |
| AI-AC-02 | Install shows size and RAM need before it starts; cancel leaves nothing behind | Not tested |
| AI-AC-03 | Model loads on first request and unloads after 2 minutes idle; memory returns to the idle level | Not tested |
| AI-AC-04 | Stop button ends an answer within 1 s | Not tested |
| AI-AC-05 | On a machine with less than 4 GB RAM, the assistant is disabled with a message | Not tested |
| AI-AC-06 | Network monitor shows no outgoing traffic during an answer | Not tested |
| AI-AC-07 | Remove deletes the model, cache, and history; disk usage returns to the pre-install value | Not tested |
| AI-AC-08 | Summarize, rewrite, and fix grammar work on the owner's test texts | Not tested |

## 8. Open Questions

- Q14: Resolved 2026-11-17 — target up to 4 to 8 GB total with the OS size; model is Qwen3.5 4B (see sections 3 and 5).
- Q23 (open, 2026-11-17): a 4B model at 4-bit needs roughly 2.5 to 3 GB of memory when loaded (estimate). The AI-08 hardware gate (4 GB RAM) is too low for it. Proposed: 8 GB RAM minimum for this model, or a smaller model for 4 GB machines.
- Whether AI-03 and AI-04 are both needed in v1 or only AI-03.
