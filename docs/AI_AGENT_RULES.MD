AUTONOMOUS EXECUTION RULES

This repository is designed to be developed by autonomous coding agents.

GENERAL RULES

Always start from this file.

Do not rely on previous chat messages.

Use the repository as the source of truth, but DO NOT load all files by default.


CONTEXT LOADING STRATEGY (CRITICAL)

You must minimize context usage.

Before reading any file:

1. Identify the current task
2. Select only the files required to solve it (max 3–5)

Only load files directly relevant to the task.

Never preload all documentation or state files.

If more context is needed, load it incrementally.

If unsure, start with the smallest possible set and expand only if necessary.


SMART STARTUP

Do NOT read all startup files by default.

Instead:

1. Always read `AI_STATE.md`

2. Then selectively read ONLY what is needed:

- Roadmap planning → `docs/ROADMAP.md`
- Active execution → `PLANS.md`
- Progress/state → `docs/WORKING_STATUS.md` or `PROJECT_STATUS.md`
- Debugging → logs
- Feature work → relevant source files

Do not read logs or additional files unless required.


FILE BUDGET

Maximum 5 files at a time.

If more are required:
- justify internally
- load incrementally (never all at once)


AUTHORITATIVE STATE

`AI_STATE.md` defines:

- current project status
- implemented behavior
- known issues
- roadmap order
- next task
- constraints

If documentation conflicts with code:
- verify against codebase
- update documentation
- keep `AI_STATE.md` accurate

Do not reload `AI_STATE.md` unless needed.


WORK MODE

Operate autonomously.

Do not narrate progress.
Do not summarize unless requested.
Do not ask for confirmation.
Do not pause between tasks.

After finishing a task, immediately continue.

Only stop on real blockers.


EXECUTION PLAN

`PLANS.md` defines the active execution queue.

Rules:

- Work through the active batch continuously
- After completing an item, mark it done and continue
- Do not yield unless blocked
- Yield only when:
  - batch is complete, or
  - a true blocker occurs

Keep `PLANS.md` updated continuously.


DECISION FLOW (CRITICAL)

For every task:

1. Understand the task
2. Select minimal required files
3. Load only those files
4. Implement solution
5. Expand context only if needed

Never assume full project context.


DECISION RULES

If multiple solutions exist, prioritize:

1. correctness
2. maintainability
3. simplicity
4. compatibility
5. modularity

Install dependencies if required.

Use standard defaults unless project defines otherwise.


PERMISSIONS (ASSUMED GRANTED)

Create, modify, rename, delete files
Run terminal commands
Install dependencies
Build the project
Run tests
Modify git repositories

Do not commit or push unless instructed.


STATE MANAGEMENT

`AI_STATE.md` must reflect the real state.

After changes, update:

- CURRENT STATUS
- CURRENT ROADMAP
- NEXT TASK
- KNOWN ISSUES (if needed)

Keep these synchronized if used:

- `PROJECT_STATUS.md`
- `docs/WORKING_STATUS.md`

Never leave state outdated.


WORKFLOW

1. Read `AI_STATE.md`
2. Identify NEXT TASK
3. Select required files (max 3–5)
4. Inspect only those files
5. Implement
6. Check for regressions or dead code
7. Update `AI_STATE.md`
8. Update `PLANS.md`
9. Continue to next item


BUILD RULES

Use existing project build scripts.

Do not invent new build processes.

If build logs exist:
- inspect before making changes
- use them to guide debugging


RUNNING THE PROJECT

Use documented run instructions.

Do not invent new run commands.


PROJECT RULES

Follow architecture and behavior defined in `AI_STATE.md`.

Do not introduce major changes unless aligned with project goals.


RECOVERY RULES

If files are missing or corrupted:
- restore from version control first

If tools are missing:
- restore project-local tools before modifying system


STOP CONDITION

Only stop if:

1. dependency cannot be installed
2. roadmap is complete
3. a true blocker prevents progress